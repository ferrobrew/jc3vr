# Stunt positions

A stunt position is the game's way of keeping a character on a moving vehicle outside its seats: standing on a car's roof, a boat's deck, or a plane's wing, or hanging from a helicopter. The character is attached to the vehicle through the vehicle's interaction graph, as a seated character is, and the vehicle carries it; its rule system plays the stunt idles and the stunt traversal. This doc covers where a vehicle offers a stunt position, how a character is attached to one, what the character's predicates read, and what is not established. The addresses and offsets are in the pyxis defs, chiefly [`Character`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character), [`Vehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle), and the `vehicle::stunt_areas` module.

## Stunt areas

A vehicle's stunt areas ([`Vehicle::m_StuntAreas`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_StuntAreas), a [`StuntAreas`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::stunt_areas::StuntAreas)) are where a character can stand on it. Each [`StuntArea`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::stunt_areas::StuntArea) is a small triangulated patch in the space of one of the vehicle's parts, with its edges, which edges are valid to step off, and its centre; neighbouring areas are welded at shared edges. The vehicle loads them from its module data, deactivates the areas of parts that break off, and moves their vertices with the part's deformation. Each area names a node of the vehicle's interaction graph (`CInteractionGraph::GetFirstNodeWithStuntArea`, by the area's name hash); a character standing in the area occupies that node, which is of the stunt-area node type.

[`GetNearestStuntArea_World`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/vehicle::stunt_areas##func-GetNearestStuntArea_World) finds the area nearest a world position: an area the position projects onto straight down its plane wins, and otherwise the nearest point on any active area's edges. It needs the vehicle's physics body, since an area is carried into the world through its part's root-space transform and the vehicle's transform at the end of the step.

A helicopter has no stunt areas to stand on. Its stunt position is a hang from the airframe, entered through the hang interaction of the vehicle's interaction context rather than an area.

## Entering

There are three ways in, all on the game update:

1. Landing on a vehicle. The on-foot locomotion task, the stunting task, and the NPC path-follow and on-spot tasks call [`AutoAttachToStuntAreaOnVehicleBelow`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-AutoAttachToStuntAreaOnVehicleBelow) each update while the character is not attached. With the character's vehicle-below flag set and that vehicle not upside down, it projects the character's position onto the nearest area and, when the projection is close enough, joins the area's node as an active user at that point, pulled in from the edge toward the node's centre when it is near an edge. The flag and the vehicle it names are character fields that are not bound; what sets them is not established, though the name and the readers point at the proxy's ground tracking.
2. A graph script. The `NGSONodes::TryAttachToClosestStuntArea` node takes a vehicle, a character (the local player by default), and an optional position, and calls [`AutoAttachToClosestStuntAreaOnVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-AutoAttachToClosestStuntAreaOnVehicle). On a helicopter that runs the hang interaction's components, which put the character in the hang; on any other vehicle it runs the stunt interaction, enters the nearest area's node, turns the character and its proxy to the vehicle's orientation, and places it at the nearest point of the area, or at the node's centre from outside the area. It fails for a helicopter without a hang interaction, and for a vehicle without areas, an area without a node, or a context without the stunt interaction.
3. The driver's seat and the grapple. The seat's stunt input moves the driver onto the roof through the rule system (`S_DRIVER_TO_STUNT_VEHICLE`), and a reel onto a vehicle ends in a stunt position ([`GHS_REELED_STUNT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookState##variant-GHS_REELED_STUNT); the grapple checks the roof for room first, `CGrapplingHook::CheckBlockedPositionForRoofStunt`, unbound). Which entry these use underneath is not established.

Both attach functions hand the character's velocity to the game (a flag of the proxy's physics input state, unbound) and set the interaction user's local matrix and the move action's capsule position to the chosen point, so from the next update the vehicle carries the character there.

## Holding

While attached, the character's place is the interaction user's local matrix in the vehicle's graph, so the vehicle's motion carries it rigidly; nothing in the proxy's support or velocity holds it there. Standing, the stunting movement task ([`NStateTask_MovementStuntingTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_MovementStuntingTask##func-Update)) moves the character over the area with the stick: it projects and clamps onto the area's edges, steps onto the next node across a welded edge, and inherits the vehicle's speed. The rule system plays the stunt idles (`S_IDLE_STUNT` and its fire variants), the stunt traversal (`S_STUNT_FWD`, `S_STUNT_RIGHT`, `S_STUNT_LEFT`, `S_STUNT_BWD`), and on a helicopter the hang (`S_IDLE_HANG_STUNT` and its aimed variants). The animation's stunt modifier ([idle-stance.md](../character/idle-stance.md)) applies while attached to a vehicle.

## The predicates

| Predicate | Reads |
| --- | --- |
| [`IsInStuntPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInStuntPosition) | attached to a vehicle, and in `S_IDLE_HANG_STUNT`, on a stunt-area node, or upside down on the grapple |
| [`IsInStuntArea`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInStuntArea) | attached to a vehicle, on a stunt-area node |
| [`IsStuntHanging`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsStuntHanging) | the attached vehicle is a [`Helicopter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Helicopter), and the rule system is in one of the stunt idles; a helicopter's stunt idle is its hang |
| [`IsStuntTraversing`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsStuntTraversing) | one of the four stunt traversal states |

None of them is a vehicle attach state ([`IsInVehicleAttachState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInVehicleAttachState) lists the riding, entry, exit, seat-switch, and mounted-gun states only). All of them walk the rule system or the interaction graph, so they are read on the game update thread.

## Leaving

A character leaves a stunt position through the rule system: the jump from the roof (`S_STUNT_TO_JUMP_EXIT_VEHICLE`), the move into the driver's seat (`S_STUNT_TO_DRIVER_VEHICLE`), a stunt traversal off an area's valid edge, a hit reaction that throws it off (the character's hit-reaction tuning has a value named for it, [`m_HitReaction_StuntFallOffImpactWeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_HitReaction_StuntFallOffImpactWeight), whose reader is not established), or the automatic exit when [`m_IsStuntAutoExitEnabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AbilityFlags##flag-m_IsStuntAutoExitEnabled) is set (the state machine's `CIsStuntAutoExitEnabledCondition`). Which input leaves the helicopter's hang is not established: the transitions out of `S_IDLE_HANG_STUNT` are in Rico's base graph (`rico_base.afsmb`), whose transition encoding is not decoded. The on-foot jump is the likely one, as it is on a roof; unverified.

## Unverified

- What sets the vehicle-below flag, and whether a character proxy standing on a moving vehicle's body sets it.
- Which input leaves the helicopter's hang, and whether the hang has an entry of its own from a grab rather than the interaction's components.
- Where on a helicopter the hang interaction places the character: it takes no position, so the hang is likely at a fixed point of the airframe rather than where the character was.
