# Seat grapple

In a vehicle seat, the grapple button fires the hook along the grapple hand's aim, and when the hook holds, what it hit is tethered to the vehicle. The base game takes no grapple fire in a seat; it ships a vehicle grapple that does this but never calls it ([grapple-pipeline.md, "The grapple in a vehicle seat"](../../engine/gameplay/grapple-pipeline.md#the-grapple-in-a-vehicle-seat)). The mod runs its steps from the input tick.

The code is `crates/jc3vr-grapple/src/grapple/seat/` (`mod.rs` for the pure step and the tick, `engine.rs` for the game calls), with the button's part in `crates/jc3vr-grapple/src/controllers/grapple_button.rs` and the aim's in `crates/jc3vr-body/src/controllers/aim/mod.rs`. The seat grapple has no switch: it acts whenever the controller scheme is in effect. Its tuning is the `seat` section of `crates/jc3vr-grapple/tuning/grapple.toml`.

## The test

The seat grapple stands in for the game's where the character rides in a seat and the game refuses the fire (`jc3vr_body::controllers::facts::seat::stands_in`; riding means the character has a vehicle and [`Character::IsInRidingInVehicleState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInRidingInVehicleState) holds). The one test decides the button, the aim, and whether a flying hook is still seated, so the button never fires the seat grapple where the aim casts nothing; in a vehicle where the game takes the fire, the button is the game's grapple.

## The aim

Every seat's animation state clears the hook's fire input, and the game's targeting does not refresh the grapple slot there, so the slot would keep the boarding point for the whole ride. So in a seat the aim update casts the grapple hand's ray itself, ignoring Rico and the vehicle and seeing through what the game's aim ray sees through, out to the grapple's range ([`GrappleTuning::m_GrappleDistance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrappleTuning##field-m_GrappleDistance)). The hit is the grapple's readout, and the overlay's highlight and beam draw it as they draw the game's target on foot. Whenever the game refuses the fire outside a seat, the readout reports nothing and marks its point as not current, so the target never freezes at a stale point.

## The button and the hook

In a seat the button's fire is the seat grapple's, and the game sees no grapple fire input:

| Press | Effect |
| --- | --- |
| A press with nothing attached | Fires at once |
| A tap with tethers attached | Fires once no second tap follows |
| A double tap with tethers attached | Releases every tether ([`GrapplingHook::ReleaseAllDualTethers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-ReleaseAllDualTethers)), as on foot |
| A hold with tethers attached | Tightens the tethers, as on foot |

A press with nothing valid under the aim is refused, and a press while the hook flies calls it back and fires again once the line is free.

The fire is the game's custom fire, [`GrapplingHook::FireHookCustom`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-FireHookCustom), at the aim's point with the vehicle ignored. The wire's near end is then detached from Rico's gauntlet and attached to the vehicle at the grapple hand's grip (the vehicle's centre when the hand is not tracked), and the hook end is sent out to the target. While the button stays held, the near end follows the grapple hand on the vehicle, so the player chooses where on the car the line pulls from.

Once the hook holds and the button is up, the wire is handed off as one of the hook's tethers ([`GrapplingHookWire::PushGrapplingHookWire`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookWire##func-PushGrapplingHookWire)), and from there it is the game's own tether: the car drags the object or is held by the wall, and the double tap releases it. A hold longer than the button's tap time ties two things instead: releasing with the aim on a second target sends the near end there, so the tether joins the first target to the second rather than to the vehicle. A long hold released with nothing under the aim calls the hook back.

A hook still flying after `seat.flight_s`, or still out when Rico leaves the seat, is called back ([`GrapplingHook::CancelActiveWire`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-CancelActiveWire)). A pause of the controllers (the mod's panel, a lost controller frame) keeps the flying hook: the next tick tethers it if it holds and Rico is still seated, or calls it back, and the flight time counts only ticks that run. Leaving the controllers (the bindings switched off, or the gamepad scheme) calls it back at once, since no later tick would. The fire is refused while a line is out or another custom fire owns the hook, and when the hook does not take it. The tether plays `Event::LineLock` on the grapple hand.

## Readouts and logs

The Status page's `Grapple` section has a `Seat grapple` row: whether the hook is flying, the counts of fires, tethers, and misses, and why the last press was refused (`nothing in range`, `no player`, `hook busy`, or `fire refused`). The debug UI's aim readout names where the grapple's point comes from: `game`, `seat`, or `stale`.

The log has, at info level under the `grapple` target, `seat grapple: fired`, `seat grapple: tether handed off` (with `dual` for a two-target tether), `seat grapple: missed`, `seat grapple: the hook was called back` (and `... on leaving the controllers`), `seat grapple: the tether was refused`, and `seat grapple: refused` with why.
