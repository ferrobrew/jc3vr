# The steering wheel and handlebars

The physical wheel and handlebars ([controllers.md, "Vehicles, mounted guns, and flight"](controllers.md#vehicles-mounted-guns-and-flight)) are placed where the driven vehicle's own are, the held wheel turns and steers as a wheel does, and the vehicle's rendered steering wheel turns with the steering. The code is in `payload/src/controllers/vehicles/`: `wheel.rs` holds the pure geometry and the override table, `fit.rs` reads the vehicle on the input tick, `steering.rs` turns the held wheel, `physical.rs` grabs and releases it, and `wheel_render.rs` turns the rendered wheel. The engine side is in [vehicle-parts.md](../../engine/gameplay/vehicle-parts.md).

## Placing the wheel

The wheel used to sit at one configured offset from the seated character's eyes for every vehicle. That missed in most cars: the hub's place relative to the head changes by about a tenth of a metre from car to car, which is more than the grab radius, the tilt ranges from nearly upright to nearly flat, and the head the offset hung from sways with the driving animation while the car's wheel does not.

Now, on each input tick while the player drives, the fit finds the vehicle's steering wheel part by name among the valid parts of its physics body ([`PfxBreakable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PfxBreakable)) and reads its model-space box. The box gives the wheel in the part's own space: the hub at its centre, the radius half its width, and the tilt from its depth against its height, clamped to a bus's flat wheel. The hub and the wheel's axes go through the part's root-space transform and the vehicle's world transform at the end of the simulation step ([`m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_WorldMatrixT1)), which is the side of the step the head pose is composed for, and then into the cockpit frame, as the mounted gun's handles are. Recomputed every tick, the wheel rides the car rather than the head, and a deformed part takes its wheel with it.

The meshes whose box misleads have an override in `WHEEL_OVERRIDES`: the circuit racer, the ladbil, the old bus, the old jeep, and the transport boat. An override gives the hub, radius, and tilt in the part's own space, so it still rides deformation. The values were placed from the boxes and the seated driver's point in the shipped data and are a starting point to tune in the headset. The table is keyed by the part's name rather than by [`m_VehicleTypeNameHash`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_VehicleTypeNameHash): the part names are read from the running game, one per model, while the type hash appears to be one per variant (the old jeep has six) and its source is unverified. The debug UI shows the type hash beside the part's name, for a later table that needs it.

## Turning the wheel

The design follows the VR wheels of other games: Sacchan's SaccGroundVehicle for VRChat (twist-tracked, hold to grip, per-hand accumulators seeded from the current steering, ±450° for a car) and BNG's VR Interaction Framework wheel (position-tracked, hold to grip, ±360°, a linear return).

A hand holds the wheel only while its grip is held. A grip press with the hand within `physical.grab_radius_m` of the rim takes the wheel, and releasing the grip lets go; so does moving further than `physical.release_distance_m` from the rim. A hand that passes through the rim, or rests on it with the grip open, takes nothing. The press is latched at its edge as for every grab volume, so a press made outside the rim and carried onto it fires the vehicle's weapon rather than grabbing.

Each holding hand turns the wheel by how far it turned since the last tick, measured one of two ways (`physical.wheel.tracking`):

- `Twist` (the default): the hand's rotation about the wheel's axis, as a hand clamped to a rim turns with it. It does not depend on where the hand is relative to the hub, so a wheel fitted a few centimetres off, a hand drifting toward the hub, or the cockpit frame swaying with the driving animation (which moves the hands relative to the car) does not turn the wheel.
- `Position`: the hand's travel around the hub in the wheel's plane. A hand within a quarter of the radius of the hub counts for nothing and one past half the radius counts fully, since the angle around the hub is unstable near it.

Only the change since the last tick counts, from where the hand took hold, so grabbing never jumps the wheel. The turns accumulate, so the wheel goes past half a turn, and a hand that lets go and takes the wheel again further round carries on from the wheel's angle (a hand-over). Two hands turn it by the mean of their turns. The wheel stops at `physical.wheel.lock_deg` either way, and a hand that turns back from the stop unwinds it at once.

The steering is the wheel's angle past `physical.wheel.deadzone_deg` (2°) as a share of the rest of the way to the lock, raised to `physical.wheel.response` (1, linear; above one is gentler around centre and still reaches full lock). The lock is 135° either way, which hands at nine and three o'clock reach without letting go. A road car turns several times that, but the game's steering is arcade and ramps by itself, and a VR wheel has no rim to push against, so a road car's range made full lock a hand-over-hand exercise.

Let go, the wheel returns to centre at `physical.wheel.return_deg_per_s` (360°/s), slowing over its last 30° to settle, and keeps steering on its way back so that the vehicle straightens with it; once centred, the stick steers again.

The holding hand is drawn on the rim at its own angle around the hub, and the highlight rings the rim point a free hand would take.

Before this, the wheel was taken by a grip press and held until the next press (so an open hand stayed on it), it turned by the hands' travel around the hub only, and it steered linearly to a full lock of 180° either way.

## The driving position

The cockpit frame's origin is the seated character's eyes as the driving animation places them, and the wheel is placed from the vehicle's part, so where the hub sits in that frame is a property of the seat: the driver animation and the wheel part between them decide it, and it differs from vehicle to vehicle. The player's tracked head adds their own posture on top. On entering a vehicle, once the driver has settled, the fit logs the hub from the character's eyes, the hub from the player's head, and how far the hub is from where the configured fallback wheel (`physical.wheel.centre_m`) sits from the eyes, which is the correction that would put every seat at one standard driving position; the vehicles section of the Controllers tab shows the same.

The correction is not applied. Applying it means offsetting the view in the vehicle, which belongs to the head pose's composition rather than to the controls; the logged values per vehicle are what a per-seat offset there would use. A second source of difference is the height remap: the view sits below the character's eyes by however far the player's head sits below their calibrated seated eye height, in every vehicle alike.

## The fallback

A vehicle with no wheel part (the modern bus, the old truck, the tanks, some boats), a wheel part that has broken off, or one with no usable model, falls back to the configured spec `physical.wheel`, given in the cockpit frame. So that it too rides the vehicle, the spec's wheel is carried into the vehicle's space once the driver has been in the vehicle long enough to have settled into the seat, and held there until the driver leaves that vehicle. Before then it hangs from the head. With `physical.from_vehicle` off, the spec is used as given, from the head, as before.

The wheel is centred on the vehicle, so it is never mirrored for a left-handed layout; neither are the handlebars. The sticks and levers still are.

## Handlebars

The ATV, the jetski, and the duck share their classes with cars and boats, but steer by handlebars. A vehicle whose parts include a steering fork (behaviour `SteeringFork`, or a part named `<model>_upper_steering`) is offered the handlebars in place of the wheel, motorcycles included. The fork's model-space box, at rest (its parent space offset under its parent's root-space transform, without the hinge's turn), places the grips' middle at the box's rear and mid-height, and the half width at half the box's width less a hand's half width. Without a usable fork, the handlebars spec places them. The game turns the forks itself, by the steering, so they are not animated by the mod.

## Turning the rendered wheel

`physical.animate_wheel` (on) turns the vehicle's steering wheel. [`Vehicle::UpdateRender_ModelMatrices`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-UpdateRender_ModelMatrices) draws each part at its root-space transform times the vehicle's render transform. A detour on it, for the driven vehicle only, turns the wheel part's [`m_RootSpaceTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PartInstance##field-m_RootSpaceTransform) about the hub by the wheel angle, calls the original, and puts the transform back. Only the drawn model turns; collision, deformation, and breaking never see it. The turn is skipped once the part, or a part it hangs from, has broken off, and when the part at the published index is no longer the part the fit found.

The angle is the held wheel's while a hand holds it or it returns to centre after being let go. Otherwise it is the wheel angle that would give the vehicle's own steering ([`m_SteeringTurn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_SteeringTurn)), through the inverse of the lock, deadzone, and response, so stick steering turns it too.

The render update runs on worker threads, so the detour reads only its own slot: an atomic with the driven vehicle's address, which every other vehicle's call compares against and passes, and a small mutex with the part, the hub and axis in the part's space, and the angle, which only the input tick's publish and the detour take, each for a copy. Nothing in the detour takes the mod's configuration or the VR state.

## Debug UI and settings

Under the vehicles section the readout shows the wheel's angle and the steering written, the driving position (above), the vehicle type hash, the wheel and fork parts found, where the wheel came from (the part, an override, or the fallback with its reason), the wheel in the part's space and as placed in the cockpit, whether the fallback is held in the vehicle's space, where the handlebars came from, and the rendered wheel's angle with a count of the render updates that turned it. The wheel's settings are `physical.wheel.lock_deg`, `response`, `deadzone_deg`, `tracking`, and `return_deg_per_s`; the placement adds `physical.from_vehicle`, `physical.animate_wheel`, and `physical.show_wheel`, which draws the wheel's rim, its axis toward the driver, and a spoke to the top of its rim in the overlay.

## Unverified

- That twist tracking feels right with the hands drawn on the rim at their own angle, and whether players drag a hand round the rim without turning it (which twist does not count, and `Position` does).
- The default lock of 135° and the return rate against the game's own steering ramp.

- The box fit's hub, radius, and tilt in each car, and the override values.
- That the handlebar grips sit at the fork box's rear and mid-height.
- The sign of [`m_SteeringTurn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_SteeringTurn) (positive is taken as right) and its range.
- That nothing else on the render worker reads the wheel part's root-space transform while the detour holds it turned.
- That the head pose is composed against the end of the simulation step, so the vehicle's `m_WorldMatrixT1` matches it at speed.
- That the settle time is long enough for the fallback to be taken with the driver seated.

## Headset checklist

1. Steering: a press on the rim takes the wheel, an open hand resting on it does not, and opening the grip lets go. Turning both hands a quarter turn steers about two thirds; full lock is reachable without letting go; a hand-over (let go with one hand, take the wheel further round) carries on without a jump; letting go returns the wheel and the car to straight. Try `Position` tracking too, and say which feels right.
2. Driving position: in several cars, note the logged "driving position" line and whether the view sits right behind the wheel; the ones that feel wrong tell whether a per-seat view correction is worth building.
3. The modern SUV, the modern saloon, the racing supercar, and the sports muscle car: the rim ring sits on the drawn wheel, the hands grab it where the wheel is drawn, and the wheel stays put while the driving animation sways the head.
4. The circuit racer, the ladbil, the old bus, and the old jeep: the overrides sit on the wheel; adjust `WHEEL_OVERRIDES` where they do not.
5. The modern bus and the old truck: the configured wheel hangs from the head for the first moment, then rides the bus.
6. A boat with a wheel (the racing boat, the small fishing boat) and the transport boat's ship's wheel.
7. The ATV, the jetski, a motorcycle, and the duck: handlebars are offered, not a wheel, and the grips sit on the drawn bars.
8. The rendered wheel turns with the held wheel, the right way, and with the stick when no hand holds it; it stops turning when it is shot off.
9. A left-handed layout: the wheel and the handlebars stay where they are; the sticks and levers swap sides.
