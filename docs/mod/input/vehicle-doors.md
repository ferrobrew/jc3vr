# Vehicle doors by hand

An empty hand opens a vehicle's doors as a real hand does: a grip on a closed door's handle area takes hold of it, a pull outward undoes the latch, and the door then swings on its hinge with the hand. Let go and it swings on; shove it shut and the game latches it. Hoods, trunks, and the other hatches a vehicle's door controller keeps open the same way. The code is `payload/src/controllers/physical_hands/doors/`; the engine side is in [vehicle-doors.md](../../engine/gameplay/vehicle-doors.md). It is on by default (`physical_hands.doors.enabled`, in the debug UI's Controllers tab under "Physical hands") and builds on the pickup and the hand bodies ([physical-hands.md](physical-hands.md#picking-things-up)). It has not yet been seen working in the headset.

The grapple snatch pulls a closed door open from range on the line, through the same reach and unlatch, with the grapple's aim point standing in for the palm ([grapple-snatch.md](grapple-snatch.md#phase-4-car-doors)).

## Finding a door

Every door of every vehicle near the player is offered by its handle, whatever the reach's rays hit (`doors/scan.rs`). The vehicles are those the game's target system keeps within 6 m of a free palm.

A door's handle comes from the door handle table when its part is in it ([`door_handle`](../../../payload/src/controllers/physical_hands/doors/geometry.rs)). The table is `tools/door_handles/door_handles.json`, embedded at build time (`doors/table.rs`) and keyed by the part's name hash, which the live part's entry in the parts resource carries ([`Part::m_name`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::Part##field-m_name)); every variant of a model shares its parts, so one entry serves them all. Each entry is a point and an outer-face normal in the part's own space, carried into the world as the door's box is. The tool places a front door's handle where the player's entry animation has its hand when it opens that door, on the door's outer surface (grade `clip`, or `clip_doubtful` when the hand is far off the door), and every other door's from its box with the hinge at its constraint point (grade `hinge`); [`tools/door_handles/README.md`](../../../tools/door_handles/README.md) has how, and the engine side is in [vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#entering-through-a-door). Every grade is used.

A door the table lacks (a DLC vehicle, or a part whose name hash is not in it) takes its handle from its part's model box alone ([`DoorBox::handle`](../../../payload/src/controllers/physical_hands/doors/geometry.rs)): the door is a panel, its thinnest extent through it; the hinge runs along one edge, and the handle is on the outer face (the face away from the vehicle's middle) at the opposite edge, a tenth of the door's run in from it, halfway along it (at mid-height for a side door, midway across for a lid). Which edge is the hinge's is read from the part's origin when it lies within a quarter of the door's run of one end, taken as the pivot; otherwise from the kind of door: a side door hinges at its front (the vehicle's -Z), a hood at its back, and a trunk, a rear door, or a hatch at its top (the front of a lid lying flat). A door part's origin is the vehicle's, not its hinge, so on the shipped vehicles the kind decides.

The first time the doors of a vehicle model are scanned, the log has, at info level, `physical hands: doors: door handles:` with the model and, for each door part, whether its handle came from the table (and the entry's grade) or from its box. A palm within `doors.handle_reach_m` (0.15 m) of a handle is offered that door, marked green on the handle. A shut door is merged into its vehicle's body, and a grip on it needs that body to prove the vehicle alive while the hand holds the latch, so a short cast from the palm to the handle, or failing that across the face at the handle, finds it; an open door's own body is taken directly. Each door near a palm is noted with its slot, whether it is shut, how far its handle is and where the handle came from, and why it was not offered (no model box, past the reach, no body found at the handle), and the notes are logged with the hand's next grip press ([grip.md](grip.md#readouts-and-logs)).

The pickup's reach rays ([physical-hands.md](physical-hands.md#picking-things-up)) also ask about every hit on a vehicle or a loosened vehicle part, before anything else, so a player standing on a car can still open its door:

- A hit on a loosened part's object ([`VehiclePartGameObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::VehiclePartGameObject)) that is one of its vehicle's doors, found through the object's vehicle and the door whose own body that object owns, is an open door, taken where the ray hit it.
- A hit on the vehicle's own body evaluates each door box in world space ([vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#the-loose-door)), using the part's model box, its root-space transform, and the vehicle's transform. The pure `doors::acquisition::panel` contract returns the reached box point, outward direction, and typed body: `PanelBody::LatchedHandle` for a closed door whose handle share meets `doors.handle_share`, or `PanelBody::OpenPanel` for an open door's own body anywhere in reach. The handle share is measured from the part's origin, its hinge pivot. Missing boxes, out-of-reach palms, a closed-panel grip short of the handle area, and inconsistent door/body states refuse acquisition, as the prior branch did. The vehicle-body ray path maps those refusals to no door target; it does not alter the separate open-part ray path.

A hit on a part object that is not a door, such as a piece broken off, is taken as any loose object. A reached door is marked green ([grip.md](grip.md#markers)).

## The latch

A grip on a closed door holds the hand at the grab point, carried with the vehicle: the hand's body is held there as a climbing hand's is held on the wall ([hand-climbing.md](hand-climbing.md#gripping)), and a probed hand is drawn there. The latch holds until the hand has pulled `doors.unlatch_pull_m` outward from the grab point, outward being from the vehicle's origin toward the door's centre (a side door's side, a hood's front and top, a trunk's back). Pushing in or sliding along the door never opens it. The door that is no longer latched when the grip looks (another hand, an animation, the AI) gives at once.

When the latch gives, the hand plays `Event::Unlatch` ([haptics.md](haptics.md)), and the door is loosened the game's own way without its motor (`doors::engine::unlatch`): [`Vehicle::OpenDoor`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-OpenDoor) puts the vehicle's body in the world and the vehicle in the update, so that the door controller keeps running for this door, the opening request it leaves is withdrawn at once so that no motor turns the hinge, and [`PfxBreakable::EnablePart`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PfxBreakable##func-EnablePart) loosens the part onto its hinge, keeping its constraint. The door's entry is marked open, as the controller marks a door it spawns.

## Swinging and shutting

From then on the hand holds the door's own body as any loose object, where it gripped: through a joint to the hand's body with the hand bodies' joint grabs ([physical-hands.md](physical-hands.md#holding-through-a-joint)), or carried otherwise. The hinge constraint removes whatever the hand asks that it forbids, so moving or turning the hand swings the door about its hinge and nothing else, with the hand's strength. Letting go leaves the door swinging with the speed the hand gave it.

A door swung shut hard latches: the door controller merges a free door that comes within a small angle of shut while swinging shut fast ([vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#the-latch)). Merging destroys the door's body, so a hand still holding it loses the hold, as any hold whose body goes away. A door eased shut stays loose against the frame.

## Tearing an open door

Door tearing is enabled by default under `physical_hands.doors.tearing` in the debug UI. An open grip must first settle with low lag. The opening pull and its latch handoff cannot directly tear the door. After settling, a sharp yank requires a rapid new increase in lag and high separating hand speed. A forceful push requires sustained grip load and outward tangential displacement at the authored hinge opening limit. Its opening direction comes from observed grip-point movement while the native hinge angle changes, in the vehicle frame. A grip taken on an already-open stationary door must swing it before a limit push can tear it. Ordinary swinging, an under-threshold pull, and a closing shove do not request tearing. The rule is implemented in `doors/tearing.rs`; the native hinge does not supply a force-break threshold.

The pickup retains the vehicle, part index, native object, and body identity through opening. Each body is processed once even with two hands. Before requesting damage, it checks current ownership, vehicle and part protection, authored detachability, body-preserving break behavior, valid non-root part identity, representable nonnegative health, and the nonparallel update phase. It calls [`PartInstance::DamagePart_ToBreakOff`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PartInstance##func-DamagePart_ToBreakOff) with player attribution and forced damage only after those checks. Force permits the authored healthy-part restriction to be bypassed. Actual invulnerability remains protected. Locked and occupied vehicles have no additional mod-side immunity. No forced native work is queued across the parallel phase.

Confirmed detachment changes all matching held doors to ordinary props. The same native body, grab frame, and existing hand joint remain in use. Native door removal and the missing-door state come from the game's break notification. A closed or replaced native body ends the old hold instead of rebinding it to a new object. Each attempted tear requires the hands to settle again. Held-body impact feedback already accepts the surviving native object/body; it has no vehicle-part type exclusion.

The debug readout counts confirmed tears and refused requests. Recorder signals are `doors.tears` and `doors.tear_refusals`; `testing/scenarios/door_tearing.toml` is a recording template, not a completed live run. Acceptance requires normal opening without tearing, a sharp yank, a sustained opening-limit push, under-threshold motion, two hands without duplicate damage, and refusal on invulnerable, mission-protected, non-detachable, disappearing, or invalid parts. Check that a torn body remains in hand, strikes scenery with impact feedback, and can be released and picked up as a prop. Check native missing-door state and vehicle interaction after tearing. Material/filter changes during native detachment and hinge-axis direction remain live acceptance risks until exercised in game.

## When the grip lets go

A grip on a latched door lets go with the grip, when the hand stands down, when the vehicle is gone (its body no longer belongs to it), when the door broke off, and when the latch did not give the door a body; each but the first logs `a grip on a latched door was lost` with the reason.

## Threading

The doors are found, gripped, and unlatched from the palms' tick (the player aim control's post-simulation update), on the game thread outside the physics step, as the door controller's own post-simulation update does it. The hand's place on the latch reaches the hand bodies' pre-simulation step through their own leaf lock, as the holds do. Every read of a vehicle's doors first checks that the vehicle is alive, through the body the reach found it on.

## Settings and readouts

The settings are `physical_hands.doors`, under the pickup's in the debug UI's Controllers tab: the switch, the reach, the handle share, and the pull that unlatches. The readout says which door a hand reaches (its slot and whether it is latched) and whether a hand grips a latch, and counts the latched grips and unlatches with the last door's slot. The `VEHICLE DOORS` status row, under `HANDS IN THE WORLD`, shows the same counts, the last door, and whether a hand is on a latch. The recorder's signals are `doors.grips`, `doors.unlatches`, and `doors.on_latch`, and the template is `testing/scenarios/vehicle_doors.toml`.

The log has, at info level, `physical hands: doors: a hand gripped a latched door` with the hand, slot, and part, `the latch gave, and the hand holds the door` with the door's body and whether it is joined, and `a grip on a latched door was lost` with the reason.

## Unverified in the headset

- Everything: that the table's handles are where the vehicles' handles are (a clip serves a class of vehicles, so on a car unlike the class's reference the hand opens the door some way from the modelled handle; the old mini's is the worst), that the box guess for a door missing from the table finds the right edge (the grip press's notes say which source was used); that the latch's pull reads right on side doors, hoods, and trunks; that the loosened door hangs on its hinge with no motor; and that a shove latches it.
- Which object a ray reports for an open door ([vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#unverified)). If it is the vehicle, the open door is found by its box, which is right only if the part's root-space transform follows the swing; if it is the part's raycast body, the hold takes the door's own body anyway.
- Whether the door controller marks the loosened door open on its own, and what an entry animation does with a door the hand holds.
- How the joint between a three-kilogram hand and a heavy door feels, and whether the hinge's limits hold against the hand's strength.

## Headset checklist

With `physical_hands.doors.enabled` and `GRIP MARKERS` on:

1. Walk up to a parked, empty modern saloon and reach toward the driver's door handle: a green marker on it, and the log's `door handles:` line for `modernsaloon` says its front doors come from the table (clip). Near the hinge: no ring.
2. Grip: `a hand gripped a latched door`, a pulse, and the rendered hand stays on the door while the controller moves. The `VEHICLE DOORS` row says `ON A LATCH`.
3. Pull outward a few centimetres: the latch gives with a strong pulse, `the latch gave, and the hand holds the door`, and the door comes open with the hand.
4. Swing the door wide and back: it turns about its hinge only. Let go while swinging: it swings on.
5. Shove the door shut hard: it latches, and the hold ends if the hand was still on it. Ease it shut: it rests against the frame, loose.
6. Open the hood from its front edge, and the trunk from its back edge.
7. Grip a door, then walk the car away (or have it driven away): the grip lets go with `the vehicle is gone` or the hand's break distance.
