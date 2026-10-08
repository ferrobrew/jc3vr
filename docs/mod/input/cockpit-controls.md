# Flight controls and cockpit buttons

The physical controls of the driver's seat ([controllers.md, "Vehicles, mounted guns, and flight"](controllers.md#vehicles-mounted-guns-and-flight)) include the plane's flight stick and throttle and the helicopter's cyclic and collective. This covers how the sticks and levers move and feel. The wheel and the handlebars are in [steering-wheel.md](steering-wheel.md). The code is in `payload/src/controllers/vehicles/physical/`: `sticks.rs` for the sticks, `levers.rs` for the levers, `cues.rs` for their haptic cues, and `control_haptics.rs` one level up for the pulses. The settings are under `controllers.vehicles.physical`, with the plane's own under `physical.flight`.

## The sticks

A stick is grabbed like every physical control: a grip press with the hand's grip within `physical.grab_radius_m` of the stick's top takes it, and releasing the grip, or moving further than `physical.release_distance_m` from it, lets go. The grab is relative. The hand's offset from the top is taken when it grabs, wherever the top is at that moment, so a hand that takes the stick off-centre, or takes it again while it is still springing back, moves it only by moving. The holding hand is drawn on the top at that offset.

The top follows the hand across the horizontal plane of the cockpit frame, up to `max_deflection_m` from centre (0.10 m for the flight stick, 0.12 m for the cyclic). The output is the deflection's share of the full deflection past `deadzone` (5%), rescaled so that it still reaches one, so the tremor of a resting hand flies nothing. Pushing forward pitches the plane's nose down and the helicopter forward, and pushing right rolls right.

Let go, a centring spring returns the stick: its deflection decays with the time constant `return_s` (0.12 s), and the stick keeps writing its value on the way back, so the plane comes back with it rather than snapping level. A thumbstick pushed out of its deadzone while the stick returns snaps the stick to centre, so the thumbstick flies at once. Before the spring, a released stick snapped to centre on the tick it was let go.

### The rudder

`physical.flight.twist_rudder` (on) works the plane's rudder by the held flight stick's twist about its shaft, the cockpit frame's vertical: turning the top to the right yaws right. The twist is measured from the hand's orientation when it took the stick, so a wrist held at any angle starts from no rudder. Twists within `twist_deadzone_deg` (6°) work nothing, and `twist_full_deg` (35°) gives a full rudder. Tilting the wrist about the other axes does not count. While the stick is held with the twist on, it replaces the tables' rudder writes, as it replaces their pitch and roll; let go, the stick's rudder stops and the thumbstick's rudder stands. The cyclic has no twist; the helicopter's yaw stays on the thumbstick.

## The levers

A lever's handle follows the hand along its axis, the collective's up and the throttle's forward, and its position over half its travel is the lever's value, `-1..=1`, with `detent` (10%) of the travel around neutral giving nothing. The collective returns to neutral when it is let go; the throttle stays where it is let go and keeps writing its value. A trigger pressed while the throttle is parked flies the plane for as long as it is pressed, and the throttle takes over again, where it was, once the trigger is let go. Before, a parked throttle overrode the trigger.

### The afterburner gate

The plane has no nitrous input of its own: its input reader ([`AirPlane::UpdateVehicleInputsFromActionMap`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane##func-UpdateVehicleInputsFromActionMap)) feeds the nitrous whenever the thrust input is above [`AirPlane::NITROUS_THRUST_THRESHOLD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane) ([input.md, "The plane's thrust and nitrous"](../../engine/gameplay/input.md#the-planes-thrust-and-nitrous)). A throttle that ran straight to full would light the afterburner in the last stretch of every push. With `physical.flight.afterburner_gate` (on), the throttle's forward half has a gate starting at `gate_at` (85%) of its travel. Below the gate the thrust rises from the detent to just under the game's threshold; in the gate it is full, and the afterburner burns where the game allows it (the nitrous upgrade). Without the upgrade, the gate is simply full thrust. Pulled back past idle, the throttle brakes, as the gamepad's left trigger does. Off, the throttle runs straight to full as before.

## Haptic cues

The held sticks and levers give their hand a cue (`PulseSource::VehicleControl`), each firing once as the control reaches it and re-armed only once the control has left it by a tenth of its travel, so a hand resting at a stop or wobbling about neutral does not buzz:

- a bump when a stick reaches its full deflection or a lever the end of its travel;
- a light detent when a lever comes back to neutral or passes it: the throttle's idle and the collective's middle;
- a firm click when the throttle goes into its gate and when it comes back out, with a little hysteresis on the way out.

One cue fires a tick. A lever that passes neutral on the tick it leaves the gate cues neutral on the next.

## Settings

The pause menu's `VR: CONTROLS` page has `FLIGHT STICK THROW` (the flight stick's full deflection, 5 to 20 cm), `TWIST FLIGHT STICK FOR RUDDER`, and `THROTTLE AFTERBURNER GATE`, under VEHICLES with the other vehicle switches. The debug UI's Controllers tab has every value: per stick the top, the full deflection, the deadzone, and the centring time; per lever its neutral, axis, travel, and detent; and under "Rudder and afterburner (plane)" the twist's deadzone and full twist and where the gate starts.

## What is logged and shown

The input tick logs `the throttle went through its afterburner gate` and `the throttle came back out of its afterburner gate` at info level. The `VR: STATUS` page's VEHICLES section has a FLIGHT CONTROLS row: `WAITING, NOT FLYING A PLANE` outside a plane's driver's seat, and in it whether the stick is held, returning, or free, the rudder, the throttle with `AFTERBURNER` while it is through the gate (or `THROTTLE BY TRIGGER` while the trigger has it), and the counts of cues and burns. The Controllers tab's vehicles section shows the same with the written rudder and the last cue. The recorder's signals are `vehicles.control_cues`, `vehicles.afterburner`, and `vehicles.afterburner_lights`, and `testing/scenarios/flight_controls.toml` is the template.

## Unverified

- That the plane's afterburner lights through the gate with the nitrous upgrade, and that the dry throttle's top, just under the threshold, does not light it.
- That the rudder's sign is right: the top turned right yaws the plane right.
- The flight stick's throw, the centring time, and the twist's range against the game's own response.
- Whether the throttle's brake half (pulled back past idle) is the brake the plane wants on the runway.

## Headset checklist

1. In a plane's driver's seat, take the throttle and push it forward: the thrust rises, and near the front a firm click marks the gate. Through it, with the nitrous upgrade, the afterburner lights; the log has `the throttle went through its afterburner gate`. Pull back out: a click, and the afterburner goes out. Push on to the stop: a bump.
2. Let go of the throttle at half: the plane keeps that thrust. Press the right trigger: the trigger flies the plane; let go of the trigger and the throttle's thrust comes back. Pull the throttle back through idle into the brake: a light click at idle.
3. Take the stick off-centre and fly: no jump at the grab; a small tremor does not move the plane; full deflection gives a bump. Let go mid-roll: the stick and the plane come back level over a fraction of a second. Take the stick again while it returns: no jump.
4. Twist the held stick to the right: the plane yaws right, and the VR: STATUS row shows a positive rudder. With `TWIST FLIGHT STICK FOR RUDDER` off, twisting does nothing and the thumbstick's rudder works.
5. In a helicopter: the cyclic springs back the same way, and the collective clicks at its middle and bumps at its ends.
