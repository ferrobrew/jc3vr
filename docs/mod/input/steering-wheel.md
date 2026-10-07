# The steering wheel and handlebars

The physical wheel and handlebars ([controllers.md, "Vehicles, mounted guns, and flight"](controllers.md#vehicles-mounted-guns-and-flight)) are placed where the driven vehicle's own are, the held wheel turns and steers as a wheel does, and the vehicle's rendered steering wheel turns with the steering. The code is in `payload/src/controllers/vehicles/`: `wheel.rs` holds the pure geometry and the override table, `fit.rs` reads the vehicle on the input tick, `steering.rs` turns the held wheel, `physical.rs` grabs and releases it, and `wheel_render.rs` turns the rendered wheel. The engine side is in [vehicle-parts.md](../../engine/gameplay/vehicle-parts.md).

## Placing the wheel

The wheel used to sit at one configured offset from the seated character's eyes for every vehicle. That missed in most cars: the hub's place relative to the head changes by about a tenth of a metre from car to car, which is more than the grab radius, the tilt ranges from nearly upright to nearly flat, and the head the offset hung from sways with the driving animation while the car's wheel does not.

Now, on each input tick while the player drives, the fit finds the vehicle's steering wheel part by name among the valid parts of its physics body ([`PfxBreakable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PfxBreakable)) and reads its model-space box. The box gives the wheel in the part's own space: the hub at its centre, the radius half its width, and the tilt from its depth against its height, clamped to a bus's flat wheel. The hub and the wheel's axes go through the part's root-space transform and the vehicle's world transform at the end of the simulation step ([`m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_WorldMatrixT1)), which is the side of the step the head pose is composed for, and then into the cockpit frame, as the mounted gun's handles are. Recomputed every tick, the wheel rides the car rather than the head, and a deformed part takes its wheel with it.

The meshes whose box misleads have an override in `WHEEL_OVERRIDES`: the circuit racer, the ladbil, the old bus, the old jeep, and the transport boat. An override gives the hub, radius, and tilt in the part's own space, so it still rides deformation. The values were placed from the boxes and the seated driver's point in the shipped data and are a starting point to tune in the headset. The table is keyed by the part's name rather than by [`m_VehicleTypeNameHash`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_VehicleTypeNameHash): the part names are read from the running game, one per model, while the type hash appears to be one per variant (the old jeep has six) and its source is unverified. The debug UI shows the type hash beside the part's name, for a later table that needs it.

## Turning the wheel

The design follows the VR wheels of other games: Sacchan's SaccGroundVehicle for VRChat (hold to grip, per-hand accumulators seeded from the current steering, ±450° for a car) and BNG's VR Interaction Framework wheel (position-tracked, hold to grip, ±360°, a linear return).

A hand holds the wheel only while its grip is held. A grip press with the hand within `physical.grab_radius_m` of the rim takes the wheel, and releasing the grip lets go; so does moving further than `physical.release_distance_m` from the rim. A hand that passes through the rim, or rests on it with the grip open, takes nothing. The press is latched at its edge as for every grab volume, so a press made outside the rim and carried onto it fires the vehicle's weapon rather than grabbing.

Each holding hand turns the wheel by how far it turned since the last tick, measured one of two ways (`physical.wheel.turned_by`):

- `Position` (the default): the hand's travel around the hub, the angle of its grip projected into the wheel's plane. This is what moves when a player turns a wheel with their arms. The angle around the hub is unstable near it, so a hand within a quarter of the radius of the hub turns the wheel by its twist instead, and from there to half the radius the travel takes over from the twist.
- `Twist`: the hand's rotation about the wheel's axis alone, as a hand clamped to a rim turns with it. It does not depend on where the hand is relative to the hub, so a wheel fitted a few centimetres off does not matter, but it reads only the wrist's roll: arms that carry the hands round the rim with the wrists held still barely turn the wheel. It was the default until a headset test found exactly that, with the grips following the hands and the wheel and the car hardly turning.

Only the change since the last tick counts, from where the hand took hold, so grabbing never jumps the wheel. Each tick's change is unwrapped and the turns accumulate, so the wheel goes past half a turn, and a hand that lets go and takes the wheel again further round carries on from the wheel's angle (a hand-over). The wheel stops at `physical.wheel.lock_deg` either way, and a hand that turns back from the stop unwinds it at once.

Two hands turn the wheel as one rigid wheel held by both would turn. The hand that turned furthest this tick leads; the other adds its turn only when it turns the other way, holding the wheel back by as much. A relaxed hand that rides the rim behind the other therefore neither halves the turn nor stops it, and two hands pulling against each other cancel. The mean of the two hands' turns, which this replaces, halved the turn whenever one hand rested; the rotation of the line between the hands has the same flaw, since one hand travelling round the rim turns that line by half as much. The jitter of two hands resting on the rim has no preferred direction, so the leader's pick does not make the wheel drift.

The steering is the wheel's angle past `physical.wheel.deadzone_deg` (2°) as a share of the rest of the way to the lock, raised to `physical.wheel.response` (1, linear; above one is gentler around centre and still reaches full lock). The lock is 135° either way, which hands at nine and three o'clock reach without letting go, and the wheel turns one to one with the hands, so the drawn wheel stays under them. A road car turns several times that, but the game's steering is arcade and ramps by itself, and a VR wheel has no rim to push against, so a road car's range made full lock a hand-over-hand exercise. The pause menu offers the lock from 60° to 180°, the response, and the tracking.

Let go, the wheel returns to centre at `physical.wheel.return_deg_per_s` (360°/s), slowing over its last 30° to settle, and keeps steering on its way back so that the vehicle straightens with it; once centred, the stick steers again.

The holding hand is drawn on the rim at its own angle around the hub, and the highlight rings the rim point a free hand would take.

Before the twist default, the wheel was taken by a grip press and held until the next press (so an open hand stayed on it), it turned by the hands' travel around the hub only, and it steered linearly to a full lock of 180° either way.

## The driving position

Where the hub sits from the seated character's eyes is a property of the seat: the driver animation and the wheel part between them decide it, and it differs from vehicle to vehicle. In the shipped data the hub sits 0.34 to 0.55 m ahead of the authored driver point and 0.55 to 0.73 m above it in the typical car, so the seats differ by a tenth of a metre or more in each direction, more than the grab radius. The player's tracked head adds their own posture on top.

The driving view (`payload/src/controllers/vehicles/driving_view.rs`, `vehicles.driving_view`) moves the view per vehicle so that the eyes sit at one standard offset from the hub. The view rather than the wheel moves because the rendered wheel cannot: it is the vehicle's own part, drawn where the vehicle puts it, so a grab volume moved away from it would have the hands closing on air beside the drawn rim.

### The standard

`vehicles.driving_view.hub_from_eyes_m` is (0, -0.30, -0.45): the hub 0.30 m below the eyes and 0.45 m ahead of them. That is the middle of a road car's ergonomic range (the wheel's centre about a quarter to a third of a metre below the driver's eyes and about half a metre ahead), a little nearer than a road car's because a VR rim holds no weight and a long reach tires the arms. The median of the cars' own seats would be the other candidate, but the shipped data does not give the character's eyes in the seat (the authored driver point is a placeholder in many cars and not the eyes in any), so the median has to come from the logged measurements below. The 0.41 m median of the hub's distance ahead of the driver point, where that point is real, agrees with the standard's fore-aft offset once the eyes are taken to sit a little behind the seat point, as a reclined driver's do.

### The correction

On each input tick while the player drives, the fit measures the hub from the character's eyes: the hub's world position less the head pose's anchor (the animated head, before the head pose moves it), in the body frame and divided by the world scale. The anchor is used rather than the cockpit origin so that neither the player's posture, the height remap, nor a correction already applied enters the measurement. Only a wheel fitted from the part or the part's override is measured; the fallback wheel says nothing about the seat.

Once the driver has settled (the same settle time as the fallback's latch), the fit averages the measurement over half a second and keeps the mean for the rest of the entry, so the driving animation's sway does not move the view. The correction is the mean's offset from the standard: moving the view by it puts the hub at the standard place. It is limited to `vehicles.driving_view.max_m` (0.25 m) in length, shortened along its direction. A seat that needs tuning gets an entry in `SEAT_OVERRIDES`, keyed by the wheel part's name like `WHEEL_OVERRIDES`, which either skips the seat or gives its correction outright (still limited). It holds only the transport boat's ship's wheel, skipped as a standing helm, until the headset says which seats need tuning.

The correction applies in cars only by default. Some boats are driven standing at the helm, where a seated standard would drag the view down by the full limit; `vehicles.driving_view.boats` turns them on. The pause menu's `VR: COMFORT` page has both switches (`FIT VIEW TO THE WHEEL` and `FIT VIEW IN BOATS TOO`).

### Applying it

The input tick publishes the driven vehicle's correction, or nothing out of a measured driver's seat. The frame half of the pose composition (`roomscale::compose_cockpit`) eases toward it over `vehicles.driving_view.blend_s` (1 s, a smoothstep restarted from the current value whenever the goal changes, so a new goal never jumps the view), and back to nothing while the seat transition is entering, shuffling, or leaving, when the setting is off, or when the input tick has stopped publishing. It adds the result to the composed cockpit position after the posture, the height remap and the seated reference, the crouch, and the chase have been resolved, so the correction neither feeds them nor is undone by them: the seated reference still follows the player's measured head height, and the correction moves the view from wherever that puts it.

The view, the hands, the grab volumes, and the head bone's target are all composed from the head pose, so they move together and the hands still meet the wheel where it is drawn. The correction is in the body frame, which rides the seat and so the vehicle.

### What is logged and shown

Once per entry, when the measurement is taken, the fit logs a `driving position` line: the vehicle type hash, the wheel part, where the wheel came from, the measured hub from the eyes, the hub from the player's head, the standard, the correction that would put the hub at the standard (`wanted`), the correction applied (`applied`, absent where the seat is not corrected), where it came from (derived, derived and limited, or an override), and whether the seat is corrected. The vehicles section of the Controllers tab shows the hub from the eyes and from the head live, the entry's measurement and correction, and the correction the view is composed with and easing toward, and has the settings under "Driving view". The camera height readout in the roomscale section lists the correction alongside the vertical offset, since it moves the camera off Rico's eyes.

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

Under the vehicles section the readout shows the wheel's angle and the steering written, the driving position and the driving view's correction (above), the vehicle type hash, the wheel and fork parts found, where the wheel came from (the part, an override, or the fallback with its reason), the wheel in the part's space and as placed in the cockpit, whether the fallback is held in the vehicle's space, where the handlebars came from, and the rendered wheel's angle with a count of the render updates that turned it. The wheel's settings are `physical.wheel.lock_deg`, `response`, `deadzone_deg`, `turned_by`, and `return_deg_per_s`; the placement adds `physical.from_vehicle`, `physical.animate_wheel`, and `physical.show_wheel`, which draws the wheel's rim, its axis toward the driver, and a spoke to the top of its rim in the overlay.

## Unverified

- That the travel tracking feels right with the hands drawn on the rim at their own angle, and that the leading hand's pick keeps the wheel with the hand that turns it without a jitter or a drift while both rest.
- The default lock of 135° and the return rate against the game's own steering ramp.

- The box fit's hub, radius, and tilt in each car, and the override values.
- That the handlebar grips sit at the fork box's rear and mid-height.
- The sign of [`m_SteeringTurn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_SteeringTurn) (positive is taken as right) and its range.
- That nothing else on the render worker reads the wheel part's root-space transform while the detour holds it turned.
- That the head pose is composed against the end of the simulation step, so the vehicle's `m_WorldMatrixT1` matches it at speed.
- That the settle time is long enough for the fallback to be taken with the driver seated.
- The standard driving position, against the logged measurements across cars, and whether the lateral correction helps or a seat off the wheel's axis is better left there.
- Which boats are driven seated, and whether the correction suits them.

## Headset checklist

1. Steering: a press on the rim takes the wheel, an open hand resting on it does not, and opening the grip lets go. Turning both hands a quarter turn round the rim steers about two thirds, and so does turning one hand a quarter turn while the other rests on the rim; the drawn wheel stays under the hands; full lock is reachable without letting go; a hand-over (let go with one hand, take the wheel further round) carries on without a jump; letting go returns the wheel and the car to straight. Try `Twist` tracking too, and say which feels right.
2. Driving view: in several cars (the modern SUV, the racing supercar, the old mini, the armoured transport), the view eases to the same place behind the wheel within a second of settling, with no jump, and the hands meet the drawn rim. Compare with the setting off. Note the logged "driving position" lines; a seat that still feels wrong gets an entry in `SEAT_OVERRIDES`, and the spread of the measured hubs says whether the standard should move toward their median.
3. Driving view and posture: sitting low or high and recentring in the seat leaves the correction alone; leaning toward the wheel still moves the view one to one; getting out eases the view back to Rico's eyes during the exit, with no jump on foot.
4. The modern SUV, the modern saloon, the racing supercar, and the sports muscle car: the rim ring sits on the drawn wheel, the hands grab it where the wheel is drawn, and the wheel stays put while the driving animation sways the head.
5. The circuit racer, the ladbil, the old bus, and the old jeep: the overrides sit on the wheel; adjust `WHEEL_OVERRIDES` where they do not.
6. The modern bus and the old truck: the configured wheel hangs from the head for the first moment, then rides the bus.
7. A boat with a wheel (the racing boat, the small fishing boat) and the transport boat's ship's wheel.
8. The ATV, the jetski, a motorcycle, and the duck: handlebars are offered, not a wheel, and the grips sit on the drawn bars.
9. The rendered wheel turns with the held wheel, the right way, and with the stick when no hand holds it; it stops turning when it is shot off.
10. A left-handed layout: the wheel and the handlebars stay where they are; the sticks and levers swap sides.
