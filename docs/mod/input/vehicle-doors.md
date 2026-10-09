# Vehicle doors by hand

An empty hand opens a vehicle's doors as a real hand does: a grip on a closed door's handle area takes hold of it, a pull outward undoes the latch, and the door then swings on its hinge with the hand. Let go and it swings on; shove it shut and the game latches it. Hoods, trunks, and the other hatches a vehicle's door controller keeps open the same way. The code is `payload/src/controllers/physical_hands/doors/`; the engine side is in [vehicle-doors.md](../../engine/gameplay/vehicle-doors.md). It is on by default (`physical_hands.doors.enabled`, `OPEN DOORS BY HAND` on the pause menu's `VR: CONTROLS` page) and builds on the pickup and the hand bodies ([physical-hands.md](physical-hands.md#picking-things-up)). It has not yet been seen working in the headset.

## Finding a door

The pickup's reach rays ([physical-hands.md](physical-hands.md#picking-things-up)) also ask about every hit on a vehicle or a loosened vehicle part, before anything else, so a player standing on a car can still open its door:

- A hit on a loosened part's object ([`VehiclePartGameObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::VehiclePartGameObject)) that is one of its vehicle's doors, found through the object's vehicle and the door whose own body that object owns, is an open door, taken where the ray hit it.
- A hit on the vehicle's own body looks at its doors' boxes in the world ([vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#the-loose-door)), each from the part's model box, its root-space transform, and the vehicle's transform. A palm within `doors.reach_m` of a closed door's box, at a point at least `doors.handle_share` of the way from the hinge to the door's far edge, reaches that door at the box's nearest point. The share is measured from the part's own origin, its hinge pivot, so a grip near the hinge takes nothing. A palm within reach of an open door's box takes that door's own body anywhere, for the case where a ray on an open door reports the vehicle.

A hit on a part object that is not a door, such as a piece broken off, is taken as any loose object. A reached door is marked green ([grip.md](grip.md#markers)).

## The latch

A grip on a closed door holds the hand at the grab point, carried with the vehicle: the hand's body is held there as a climbing hand's is held on the wall ([hand-climbing.md](hand-climbing.md#gripping)), and a probed hand is drawn there. The latch holds until the hand has pulled `doors.unlatch_pull_m` outward from the grab point, outward being from the vehicle's origin toward the door's centre (a side door's side, a hood's front and top, a trunk's back). Pushing in or sliding along the door never opens it. The door that is no longer latched when the grip looks (another hand, an animation, the AI) gives at once.

When the latch gives, the hand plays `Event::Unlatch` ([haptics.md](haptics.md)), and the door is loosened the game's own way without its motor (`doors::engine::unlatch`): [`Vehicle::OpenDoor`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-OpenDoor) puts the vehicle's body in the world and the vehicle in the update, so that the door controller keeps running for this door, the opening request it leaves is withdrawn at once so that no motor turns the hinge, and [`PfxBreakable::EnablePart`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PfxBreakable##func-EnablePart) loosens the part onto its hinge, keeping its constraint. The door's entry is marked open, as the controller marks a door it spawns.

## Swinging and shutting

From then on the hand holds the door's own body as any loose object, where it gripped: through a joint to the hand's body with the hand bodies' joint grabs ([physical-hands.md](physical-hands.md#holding-through-a-joint)), or carried otherwise. The hinge constraint removes whatever the hand asks that it forbids, so moving or turning the hand swings the door about its hinge and nothing else, with the hand's strength. Letting go leaves the door swinging with the speed the hand gave it.

A door swung shut hard latches: the door controller merges a free door that comes within a small angle of shut while swinging shut fast ([vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#the-latch)). Merging destroys the door's body, so a hand still holding it loses the hold, as any hold whose body goes away. A door eased shut stays loose against the frame.

## When the grip lets go

A grip on a latched door lets go with the grip, when the hand stands down, when the vehicle is gone (its body no longer belongs to it), when the door broke off, and when the latch did not give the door a body; each but the first logs `a grip on a latched door was lost` with the reason.

## Threading

The doors are found, gripped, and unlatched from the palms' tick (the player aim control's post-simulation update), on the game thread outside the physics step, as the door controller's own post-simulation update does it. The hand's place on the latch reaches the hand bodies' pre-simulation step through their own leaf lock, as the holds do. Every read of a vehicle's doors first checks that the vehicle is alive, through the body the reach found it on.

## Settings and readouts

The settings are `physical_hands.doors`, under the pickup's in the debug UI's Controllers tab: the switch, the reach, the handle share, and the pull that unlatches. The readout says which door a hand reaches (its slot and whether it is latched) and whether a hand grips a latch, and counts the latched grips and unlatches with the last door's slot. The `VR: STATUS` page's `VEHICLE DOORS` row, under `HANDS IN THE WORLD`, shows the same counts, the last door, and whether a hand is on a latch. The recorder's signals are `doors.grips`, `doors.unlatches`, and `doors.on_latch`, and the template is `testing/scenarios/vehicle_doors.toml`.

The log has, at info level, `physical hands: doors: a hand gripped a latched door` with the hand, slot, and part, `the latch gave, and the hand holds the door` with the door's body and whether it is joined, and `a grip on a latched door was lost` with the reason.

## Unverified in the headset

- Everything: that the reach finds closed doors' handle areas on real vehicles, which depends on each door's part origin being its hinge pivot; that the latch's pull reads right on side doors, hoods, and trunks; that the loosened door hangs on its hinge with no motor; and that a shove latches it.
- Which object a ray reports for an open door ([vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#unverified)). If it is the vehicle, the open door is found by its box, which is right only if the part's root-space transform follows the swing; if it is the part's raycast body, the hold takes the door's own body anyway.
- Whether the door controller marks the loosened door open on its own, and what an entry animation does with a door the hand holds.
- How the joint between a three-kilogram hand and a heavy door feels, and whether the hinge's limits hold against the hand's strength.

## Headset checklist

With `OPEN DOORS BY HAND` and `GRIP MARKERS` on:

1. Walk up to a parked, empty car and reach toward the driver's door near its back edge: a green marker at the door. Near the hinge: no ring.
2. Grip: `a hand gripped a latched door`, a pulse, and the rendered hand stays on the door while the controller moves. The `VEHICLE DOORS` row says `ON A LATCH`.
3. Pull outward a few centimetres: the latch gives with a strong pulse, `the latch gave, and the hand holds the door`, and the door comes open with the hand.
4. Swing the door wide and back: it turns about its hinge only. Let go while swinging: it swings on.
5. Shove the door shut hard: it latches, and the hold ends if the hand was still on it. Ease it shut: it rests against the frame, loose.
6. Open the hood from its front edge, and the trunk from its back edge.
7. Grip a door, then walk the car away (or have it driven away): the grip lets go with `the vehicle is gone` or the hand's break distance.
