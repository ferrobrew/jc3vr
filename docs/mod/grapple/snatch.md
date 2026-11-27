# The grapple snatch

The empty grapple hand pulls things to Rico. Pointed at something out of reach, a squeeze of the grip and a yank back fire the game's hook at it and pull it into a hand: a weapon from the ground, a rack or a locker, a character's hands, or a holster, a loose object, a grenade, or a placed charge. A car door is pulled open, and a heavy object or a ragdoll is swung on the line instead. The rest of the grapple is [grapple.md](grapple.md).

The snatch keeps the game's grapple wherever it can: its aim, its hook, line, and flight from Rico's left forearm, and its pickup and steal rules. The mod adds the trigger, the pull, and the arrival. The code is `controllers::snatch` in `jc3vr-grapple`, with the aim patch in `jc3vr_body::controllers::aim::grapple_weapons`, and the keys are in `crates/jc3vr-grapple/tuning/snatch.toml` ([tuning.md](../tuning.md)). Why the game's grapple never hooks a weapon is [grapple-pipeline.md, "Weapons and the grapple"](../../engine/gameplay/grapple-pipeline.md#weapons-and-the-grapple). An arrival lands through the [two-handed inventory](../input/two-handed-inventory.md)'s takes, never the game's equip. The snatch has no switch and needs the controller scheme.

## Targeting

The game's aim ray passes through weapons, so a detour of [`PlayerAimControl::CheckDirectTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-CheckDirectTargets) recasts the grapple's pass of it ([grapple.md](grapple.md#the-aim)) on [`VEHICLE_RAYCAST`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxLayer##variant-VEHICLE_RAYCAST), which meets weapons, and writes a weapon it finds into the ray's closest hit ([`CastRayInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::CastRayInfo)), so the game treats it as any hit. The layer table is never patched, since every query on the layer shares it, and nothing behind a wall is found.

The candidates (`snatch::target`) are guns in characters' hands and holsters, which the steal notes out to `range_m` ([physical-weapons.md](../input/physical-weapons.md#taking-a-gun-from-a-character)), within `cone_deg`; weapons, loose objects, grenades, and placed charges lying in the world, found by their broad-phase boxes near the ray within a cone that widens with distance by `floor_cone_widen_deg`, since a gun lying flat at range is hard to hold a hand on; weapons a rack or locker holds out; and what the aim hits itself, including a car door's handle. Each off-ray candidate needs a line of sight, and the smallest angle wins. A target just aimed at is kept for `retain_s` across the grip's edge (`snatch::prearm`). The chosen target shows a ring in its kind's colour. A gun still on its mount is never offered.

In the grip's targeting ([grip.md](../input/grip.md)) the snatch is the lowest tier, `Tier::Remote`, taken only when nothing at all is in reach. It is offered only to the grapple hand, empty, with the hook free and the grapple button up, on foot or riding in a car seat, outside any traversal (`snatch::available_in`).

## The interaction

1. Aim: the ring shows the target.
2. Grip: squeezing arms the snatch, shown by a cyan dot and a line from the hand (`snatch::latch`).
3. Yank: a pull of the hand back along its aim while the grip is held fires it, with a detector (the `yank` section) gentler than the zip's ([grapple.md](grapple.md#the-hand-on-the-line)), since the grip already armed it. A punch never snatches.
4. Flight: the hook bites, and the line pulls the target toward a hand as a real body that spins and collides. A gun turns in the air to arrive in the pose the catching hand holds it in.
5. Catch: the line steers the target to the free, empty hand nearest its path, so the player can pull with the grapple hand and hold out the other. Just short of the hand the line goes slack, and a hand catches when its grip closes on the target in the catch zone or a moment after. Otherwise it flies past and drops.

Opening the grip cancels, and in flight the target keeps its momentum and falls; leaving the places the snatch works cancels too. While the line is out the other hand's grip is reserved for the catch (`snatch::reserved_hands`): it takes nothing and drives no game action, except a grip on the line itself ([grapple.md](grapple.md#the-free-hand-on-the-line)). The controls summary shows the snatch's steps ([bindings.md](../input/bindings.md#the-controls-summary)). Haptics: `Event::SnatchArm`, `SnatchFire`, `SnatchHit`, `Sustain::GrappleTension` while the line pulls, `Event::SnatchCatch` on the catching hand, and `SnatchRefused` for a refusal or miss ([haptics.md](../input/haptics.md)).

## The pull and the catch

Before anything flies, the source is freed through the hands' take ([physical-weapons.md, "Taking a weapon from a source"](../input/physical-weapons.md#taking-a-weapon-from-a-source)) and the pickup's rules are checked, so a snatch the hand could not finish never fires. The hook goes out through [`GrapplingHook::FireHookCustom`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-FireHookCustom) with a target from [`HookTargetInfo::SetupTargetFromGameObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::HookTargetInfo##func-SetupTargetFromGameObject) and the ghost-wire flag, since a ghost wire whose casts miss, as they do on a weapon, attaches to the target object itself.

At the bite ([`WireEnd::m_WireEndState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::WireEnd##field-m_WireEndState)) the native rope is taken off the line (`sling_hooks::exclude`), and `pull::pull` drives the body through the physical hands' hold solve toward the receiving wrist with a strength of its mass times `line_accel_mps2`. Far out it pulls at the hook's point on an `arc` above the wrist, which spins it; within `approach_m` the speed falls toward `arrival_speed_mps` and the pull moves to the centre. A target heavier than `heavy_mass_kg` gets a wider catch zone and a slower line. The line never brakes to a stop, so the target arrives as a thrown thing does.

The receiver (`catch::receiver`) is the free, empty hand whose wrist is nearest the segment from the target to the eye, with hysteresis. Within the distance the target flies in `coast_s` the line goes slack. The catch is swept along the path since the last tick (`catch::CatchWatch`): a hand catches when its grip is closed with the target within `catch_zone_m` of its wrist, or up to `catch_grace_s` after. The grapple hand, whose grip holds the line, catches only as the receiver, and nothing draws a slack target toward a hand. A slack target that recedes, stops, or flies past `max_slack_s` is a miss; one refused by the loadout table on the catch drops at the hand. A stall (`stall_distance_m` over `stall_window_s`), a target beyond the wire's [`m_MaxLength`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookWire##field-m_MaxLength), a hook flying past `attach_timeout_s`, or a wire the game ends also fail it. Targets are held by a strong reference so the despawn never takes one mid-flight, except a pooled C4 charge.

`FireHookCustom` refuses while a line is out or the hook is busy, and the mod refuses before it while a normal fire is pending, which `FireHookCustom` does not test; dual tethers do not block it. While the line is out the bindings hold the grapple button back from the game, and the snatch ends its own wire on every exit, after checking it is still the hook's active one.

A gun lands ready to shoot (`snatch::landing`, `snatch::arrival`). Each tick the landing knows where the receiver will hold the gun, the wrist target extrapolated to the catch composed with the canonical grip (`grip::canonical_on_wrist_frame`), and steers the body's spin toward the turn still to make over the time to go (`landing::turn_drive`, the `landing` section), on after the line goes slack. The position is corrected only while the line pulls, across it and within `landing.lateral_window_m`, and the take's slide covers what is left.

## Targets

| Kind | Freeing | Finish |
| --- | --- | --- |
| Weapon on the ground | none | the direct pickup |
| Gun in a character's hands | the steal's drop and stagger | the direct pickup |
| Gun holstered on a character | dropped, the character keeping what it wields | the direct pickup |
| Gun on a rack or in a locker | dropped where it hangs | the direct pickup |
| Loose object a hand can carry | turned loose at the bite if placed | the hand's ordinary hold |
| Grenade lying in the world | none | taken into the hand through the loadout |
| Placed C4 charge | its body put into the simulation | the hand's take of a placed charge |
| Heavy loose object or ragdoll | none | swung on the line |
| Car door | unlatched once the hook holds | pushed outward to swing |

A holstered gun is dropped as the player's replaced weapon is ([`WeaponBase::OnDrop`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-OnDrop), then [`Inventory::DropWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-DropWeapon)), and a freed gun is fired at once its body can be read, within `free_timeout_s`.

A rack's weapon has no body and the aim never hits it, so `hold::rack::remote` in `jc3vr-weapons` notes from the weapon render detour, by weak reference, the weapons a placement rule holds out: one in the rack state, or one lying where its rule set it down with no holder ([`WeaponBase::m_SpawnedFromRule`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_SpawnedFromRule), a locker's). Firing frees it through the take a hand makes ([physical-weapons.md, "Taking a weapon off a rack"](../input/physical-weapons.md#taking-a-weapon-off-a-rack)): a finite weapon gets [`WeaponBase::PickupStart`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-PickupStart) and drops; an unlimited rack ([`WeaponBase::m_InfinitePickup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_InfinitePickup)) spawns a copy the snatch waits for up to `hold.rack.copy_wait_ms`; an unlimited locker weapon is not offered, since its copy would drop inside the original; and a pistol pair gives up the pistol aimed at, claimed at once ([physical-weapons.md, "Paired rack pistols"](../input/physical-weapons.md#paired-rack-pistols)).

A loose object is what the hand's own classification calls one ([physical-hands.md, "Picking things up"](../input/physical-hands.md#picking-things-up)), not held by the other hand. A placed object at rest has no dynamic body for the box search, so it is offered only under the aim's exact hit and turned loose at the bite ([havok-bodies.md](../../engine/physics/havok-bodies.md#placed-bodies-becoming-dynamic)); on the catch it is handed to the hand's ordinary hold, whose release throws it.

An inert grenade of the loadout is anchored by its own body, since a sight cast rarely meets one, and taken through the loadout ([grenades.md, "Dropped grenades"](../input/grenades.md#dropped-grenades)); a finite native grenade pickup is taken as a hand closing on it would. A live grenade or one on a display is not a target.

A placed C4 charge (`c4::placed::charges`) has its body put into the simulation by [`PlantedExplosive::DropExplosive`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::PlantedExplosive##func-DropExplosive) at the yank, and its stall window is scaled by `charge_stall_scale`, since it comes off its surface slowly. It stays live and detonatable in flight, and a miss leaves it in the world. Only a catch takes it (`c4::placed::take`), priming a replacement in the hand before removing the original; flight holds only the pool slot and allocation identity, so a charge recycled in flight is refused cleanly.

A car door of the vehicle under the aim is found by the doors' own reach within `door_reach_m` of the handle ([vehicle-doors.md](../input/vehicle-doors.md)), unless the vehicle is faster than `door_max_vehicle_speed_mps`. Once the hook holds, the door is unlatched as a hand's pull does, pushed outward by its mass times `door_swing_mps`, and the wire ends.

## Swinging on the line

A heavy object or a ragdoll is never pulled into the hand: the held line swings it from the grapple hand, to be dragged, whirled, or lifted and let go by opening the grip (`snatch::sling`, `snatch::brace`, the `swing` section). Three things swing:

- a loose object too heavy for one hand and no heavier than `swing.ceiling_kg`, above which nothing is offered (vehicles and parts on them never are, since the hook would select the vehicle's body);
- a ragdolling character under the ceiling, hooked at the bone nearest the hit ([character-grabs.md](../input/character-grabs.md#reaching-a-character)), pulled by an impulse split between the hooked body (`remote.ragdoll_lead_share`) and every body by mass, since the ragdoll's constraints cannot pass a whole body's momentum within one step, and reeled in to arm's reach (the `ragdoll` section) for the free hand to take;
- a weapon or light object while the grapple button is held after the grip; releasing the button resumes the pull to the hand.

The line is the lock's rope ([grapple.md](grapple.md#the-lock)) with the tracked wrist as its fixed end, its strength scaling with the target's mass (`strength_n`, `mass_scaling`), so a barrel takes a real swing and a rifle swings briskly. It writes only velocity, and the target collides with everything, Rico included. The held length reels in toward `swing_length_m` but never faster than the target comes, and never pays out. The device proxy's visual getters get the wrist's frame, so the game draws the line from the tracked forearm, and the native constraint and tension callbacks are excluded (`sling_hooks`), so the target takes no second rope force. Overstretch (`max_stretch_m`), an anchor jump, tracking loss, a lost line, an occupied hand, or a menu ends it.

The grapple button is the snatch's from the grip's arming until the catch or miss, so a press under an armed heavy snatch does nothing rather than fire the game's grapple. The other hand can brace the grapple hand by gripping within `brace_take_m` of its wrist, raising the line's strength by `brace_factor` and anchoring between the hands until they drift `brace_break_m` apart, or take the line ahead of the wrist, which makes it the anchor (`Brace::step_line`) so its pull draws the target in; neither change of anchor reads as hand motion. The latch draws the swinging line coloured and widened by tension, with a ring where the held length ends.

## The grapple button at a gun

With the aim seeing weapons, the button acts on a gun as the game does on any light object, with one difference (`snatch::hooks`, only while the bindings inject). A detour of [`WireEnd::SetExtend`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::WireEnd##func-SetExtend) makes a hook fired at a weapon a ghost wire, since the hook's casts never hit a weapon's body; that covers both ends of a tether, so a gun can be tethered to a car. A tap tosses the gun to Rico's feet rather than Rico to the gun: where [`GrapplingHook::OnPreSimUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-OnPreSimUpdate) would start a reel on a weapon, a detour ends the wire and the next input tick tosses the gun on an arc (the `toss` section).

## Sounds and readouts

The snatch plays the grapple's own effects (`snatch::juice`; [grapple-pipeline.md, "The hook's sounds and effects"](../../engine/gameplay/grapple-pipeline.md#the-hooks-sounds-and-effects)): the custom fire plays no fire animation, so the mod plays the fire's [`GRAPPLE_FIRE_USAGE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##const-GRAPPLE_FIRE_USAGE), the bite's [`WireEnd::PlayImpactEffect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::WireEnd##func-PlayImpactEffect), the reel's loop through a detour of [`GrapplingHook::UpdateLoopingEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-UpdateLoopingEffects), and the retract's one-off. The pre-fire sound is not played, since which point of the fire it belongs to is unverified.

The Status page's `Snatch` row shows the phase (idle, armed, freeing, hook out, pulling, inbound, swinging), the offered target or why none is offered, the swing's length, tension, and brace, and the failures and tosses. The log's `controllers` target follows each snatch from `snatch: armed; a yank fires` to the catch, `snatch: failed`, or `snatch: cancelled`.

## Unverified

- How the AI reacts to a disarm or a rack weapon taken from range, and whether it reserves a dropped weapon it walks toward.
- Whether a target caught close to the body hits Rico's collision proxy first.
- Whether a swung barrel's impact can explode it, and whether the player counts as its instigator.
- Whether the hook is ever parked off inactive with no wire, which would refuse the custom fire.
