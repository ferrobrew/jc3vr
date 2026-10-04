# The steering wheel and handlebars

The physical wheel and handlebars ([controllers.md, "Vehicles, mounted guns, and flight"](controllers.md#vehicles-mounted-guns-and-flight)) are placed where the driven vehicle's own are, and the vehicle's rendered steering wheel turns with the steering. The code is in `payload/src/controllers/vehicles/`: `wheel.rs` holds the pure geometry and the override table, `fit.rs` reads the vehicle on the input tick, and `wheel_render.rs` turns the rendered wheel. The engine side is in [vehicle-parts.md](../../engine/gameplay/vehicle-parts.md).

## Placing the wheel

The wheel used to sit at one configured offset from the seated character's eyes for every vehicle. That missed in most cars: the hub's place relative to the head changes by about a tenth of a metre from car to car, which is more than the grab radius, the tilt ranges from nearly upright to nearly flat, and the head the offset hung from sways with the driving animation while the car's wheel does not.

Now, on each input tick while the player drives, the fit finds the vehicle's steering wheel part by name among the valid parts of its physics body ([`PfxBreakable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PfxBreakable)) and reads its model-space box. The box gives the wheel in the part's own space: the hub at its centre, the radius half its width, and the tilt from its depth against its height, clamped to a bus's flat wheel. The hub and the wheel's axes go through the part's root-space transform and the vehicle's world transform at the end of the simulation step ([`m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_WorldMatrixT1)), which is the side of the step the head pose is composed for, and then into the cockpit frame, as the mounted gun's handles are. Recomputed every tick, the wheel rides the car rather than the head, and a deformed part takes its wheel with it.

The meshes whose box misleads have an override in `WHEEL_OVERRIDES`: the circuit racer, the ladbil, the old bus, the old jeep, and the transport boat. An override gives the hub, radius, and tilt in the part's own space, so it still rides deformation. The values were placed from the boxes and the seated driver's point in the shipped data and are a starting point to tune in the headset. The table is keyed by the part's name rather than by [`m_VehicleTypeNameHash`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_VehicleTypeNameHash): the part names are read from the running game, one per model, while the type hash appears to be one per variant (the old jeep has six) and its source is unverified. The debug UI shows the type hash beside the part's name, for a later table that needs it.

## The fallback

A vehicle with no wheel part (the modern bus, the old truck, the tanks, some boats), a wheel part that has broken off, or one with no usable model, falls back to the configured spec `physical.wheel`, given in the cockpit frame. So that it too rides the vehicle, the spec's wheel is carried into the vehicle's space once the driver has been in the vehicle long enough to have settled into the seat, and held there until the driver leaves that vehicle. Before then it hangs from the head. With `physical.from_vehicle` off, the spec is used as given, from the head, as before.

The wheel is centred on the vehicle, so it is never mirrored for a left-handed layout; neither are the handlebars. The sticks and levers still are.

## Handlebars

The ATV, the jetski, and the duck share their classes with cars and boats, but steer by handlebars. A vehicle whose parts include a steering fork (behaviour `SteeringFork`, or a part named `<model>_upper_steering`) is offered the handlebars in place of the wheel, motorcycles included. The fork's model-space box, at rest (its parent space offset under its parent's root-space transform, without the hinge's turn), places the grips' middle at the box's rear and mid-height, and the half width at half the box's width less a hand's half width. Without a usable fork, the handlebars spec places them. The game turns the forks itself, by the steering, so they are not animated by the mod.

## Turning the rendered wheel

`physical.animate_wheel` (on) turns the vehicle's steering wheel. [`Vehicle::UpdateRender_ModelMatrices`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-UpdateRender_ModelMatrices) draws each part at its root-space transform times the vehicle's render transform. A detour on it, for the driven vehicle only, turns the wheel part's [`m_RootSpaceTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PartInstance##field-m_RootSpaceTransform) about the hub by the wheel angle, calls the original, and puts the transform back. Only the drawn model turns; collision, deformation, and breaking never see it. The turn is skipped once the part, or a part it hangs from, has broken off, and when the part at the published index is no longer the part the fit found.

The angle is the held wheel's while a hand holds it. Otherwise it is the vehicle's own steering ([`m_SteeringTurn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_SteeringTurn)) over the wheel's full-steer angle, so stick steering turns it too.

The render update runs on worker threads, so the detour reads only its own slot: an atomic with the driven vehicle's address, which every other vehicle's call compares against and passes, and a small mutex with the part, the hub and axis in the part's space, and the angle, which only the input tick's publish and the detour take, each for a copy. Nothing in the detour takes the mod's configuration or the VR state.

## Debug UI and settings

Under the vehicles section the readout shows the vehicle type hash, the wheel and fork parts found, where the wheel came from (the part, an override, or the fallback with its reason), the wheel in the part's space and as placed in the cockpit, whether the fallback is held in the vehicle's space, where the handlebars came from, and the rendered wheel's angle with a count of the render updates that turned it. The settings add `physical.from_vehicle`, `physical.animate_wheel`, and `physical.show_wheel`, which draws the wheel's rim, its axis toward the driver, and a spoke to the top of its rim in the overlay.

## Unverified

- The box fit's hub, radius, and tilt in each car, and the override values.
- That the handlebar grips sit at the fork box's rear and mid-height.
- The sign of [`m_SteeringTurn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_SteeringTurn) (positive is taken as right) and its range.
- That nothing else on the render worker reads the wheel part's root-space transform while the detour holds it turned.
- That the head pose is composed against the end of the simulation step, so the vehicle's `m_WorldMatrixT1` matches it at speed.
- That the settle time is long enough for the fallback to be taken with the driver seated.

## Headset checklist

1. The modern SUV, the modern saloon, the racing supercar, and the sports muscle car: the rim ring sits on the drawn wheel, the hands grab it where the wheel is drawn, and the wheel stays put while the driving animation sways the head.
2. The circuit racer, the ladbil, the old bus, and the old jeep: the overrides sit on the wheel; adjust `WHEEL_OVERRIDES` where they do not.
3. The modern bus and the old truck: the configured wheel hangs from the head for the first moment, then rides the bus.
4. A boat with a wheel (the racing boat, the small fishing boat) and the transport boat's ship's wheel.
5. The ATV, the jetski, a motorcycle, and the duck: handlebars are offered, not a wheel, and the grips sit on the drawn bars.
6. The rendered wheel turns with the held wheel, the right way, and with the stick when no hand holds it; it stops turning when it is shot off.
7. A left-handed layout: the wheel and the handlebars stay where they are; the sticks and levers swap sides.
