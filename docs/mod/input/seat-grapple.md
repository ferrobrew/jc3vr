# Seat grapple

In a vehicle seat, the grapple button fires the hook along the off-hand's aim, and when the hook holds, what it hit is tethered to the vehicle. The base game takes no grapple fire in a seat; it ships a vehicle grapple that does exactly this but never calls it ([grapple-pipeline.md, "The grapple in a vehicle seat"](../../engine/gameplay/grapple-pipeline.md#the-grapple-in-a-vehicle-seat)). The mod runs the same steps from the input tick.

The code is `payload/src/grapple/seat/`, with the button's part in `payload/src/controllers/bindings/grapple_button.rs` and the aim's in `payload/src/controllers/aim/mod.rs`.

## The aim

Every seat's animation state clears the hook's fire input, and the game's targeting does not refresh the grapple slot there, so the slot would keep the boarding point for the whole ride. In a seat (the character has a vehicle and is in one of the riding states, as [`Character::IsInRidingInVehicleState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInRidingInVehicleState) says) with the game refusing the fire (`grapple::seat::stands_in`), the aim update casts the off-hand's grapple ray itself, ignoring Rico and the vehicle and seeing through what the game's aim ray sees through, out to the grapple's range ([`GrappleTuning::m_GrappleDistance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrappleTuning##field-m_GrappleDistance)). The hit is the grapple's readout: valid while there is one, tetherable when it is an object. The overlay's highlight and beam draw it as they draw the game's target on foot.

With the seat grapple off, or whenever the game refuses the fire outside a seat, the readout reports nothing hit or valid and marks its point as not current, and the beam is not drawn: the target never freezes at a stale point ([controllers.md](controllers.md)).

## The button

In a seat, by the same test as the aim's, the button's fire is the seat grapple's, and the game sees no grapple fire input. The one test decides the button, the aim, and whether a flying hook is still seated, so the button never fires the seat grapple where the aim casts nothing; in a vehicle where the game takes the fire, the button is the game's grapple:

| Press | Effect |
|---|---|
| A press with nothing attached | Fires the seat grapple at once |
| A tap with tethers attached | Fires the seat grapple once no second tap follows |
| A double tap with tethers attached | Releases every tether, as on foot |
| A hold with tethers attached | Tightens the tethers, as on foot |

A press while the hook flies does nothing. A press with nothing valid under the aim is refused.

## The hook

The fire is the game's vehicle grapple's: [`GrapplingHook::FireHookCustom`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-FireHookCustom) at the aim's point, ignoring the vehicle, with no game object and no bone, not a ghost wire, so the hook flies to the point and holds whatever it meets there. When its far end holds, [`GrapplingHook::TetherActiveWireToVehicleCOM`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-TetherActiveWireToVehicleCOM) moves the wire's near end from Rico's gauntlet to the vehicle's centre of mass and makes it a tether. From there it is the game's own tether: the car drags the object or is held by the wall, the double tap releases it, and tightening squeezes it.

A hook still flying after two seconds, or one still out when Rico leaves the seat, is called back ([`GrapplingHook::CancelActiveWire`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-CancelActiveWire)). A pause of the controllers (the mod's panel, a lost controller frame) keeps the flying hook: the next tick tethers it if it holds and Rico is still seated, or calls it back otherwise, and the two seconds count only ticks that run. Leaving the controllers (the bindings switched off, or the gamepad scheme) calls it back at once, since no later tick would. The fire is refused while a line is out or a fire is under way, and when the hook does not take it.

The tether plays `Event::LineLock` on the grapple hand.

## Settings and readouts

The switch is in the debug UI's Grapple feature, under Movement/vehicles: grapple from a seat (`controllers.grapple.seat`, on by default). The Status page's `GRAPPLE` section has a `SEAT GRAPPLE` row: whether the hook is flying, the counts of fires, tethers, and misses, and why the last press was refused. The debug UI's aim readout names where the grapple's point comes from: `game`, `seat`, or `stale`.

The log has, at info level under the `grapple` target, `seat grapple: fired` with the point, `seat grapple: tethered to the vehicle`, `seat grapple: missed`, `seat grapple: the hook was called back` (and `... on leaving the controllers`), `seat grapple: the tether was refused`, and `seat grapple: refused` with why.

## Headset checklist

1. Sit in a car's driver's seat and point the off-hand at a wall or a parked car: the grapple highlight and beam follow the hand, not the point where Rico got in.
2. Press the grapple button: the hook flies to the point, `seat grapple: fired` then `tethered to the vehicle` are logged, and the lock pulse is felt. Drive off: the car is held by the wall, or drags the parked car.
3. Double tap the grapple button: the tether is released. With tethers out, hold the button: they tighten.
4. Fire at nothing in range: the press is refused (`nothing in range`), nothing flies.
5. Fire, and leave the seat while the hook flies: the hook is called back.
6. Do the same from a passenger seat, a motorcycle, a boat, and a helicopter's seat.
7. Fire, and open the mod's panel while the hook flies, then close it: the hook tethers if it held, or is called back, and the next press fires again rather than being refused as `hook busy`. Fire and switch to the gamepad scheme while it flies: the hook is called back.
8. Switch the seat grapple off: in a seat the highlight and beam disappear, and the button fires nothing.
9. On foot, fire the grapple: the highlight and beam vanish during the fire and come back with the game's target after it.

## Unverified in the headset

- That the hook's wire updates and attaches from a seat as it does on foot, and that nothing in the seat's states cuts it.
- What the tether does to the vehicle's handling, and whether the game's tether strength and break suit a car pulling on it.
- Whether the grapple comfort hold or the hand anchor react to a wire fired from a seat.
- That `IsInRidingInVehicleState` holds in every seat class the mode detector calls a seat.
