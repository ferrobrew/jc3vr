# Flight controls and cockpit buttons

The physical controls of the driver's seat ([controllers.md, "Vehicles, mounted guns, and flight"](controllers.md#vehicles-mounted-guns-and-flight)) include the plane's flight stick and throttle and the helicopter's cyclic and collective, and the cockpit's buttons are worked by hand alongside the controller's. This covers how the sticks and levers move and feel, and the horn, the nitrous, and the landing gear. The wheel and the handlebars are in [steering-wheel.md](steering-wheel.md). The code is in `payload/src/controllers/vehicles/physical/`: `sticks.rs` for the sticks, `levers.rs` for the levers, `cues.rs` for their haptic cues, and `control_haptics.rs` one level up for the pulses. The switches are under `controllers.vehicles.physical`, and the placement and tuning are fixed in `PhysicalControlsTuning` (`controllers::vehicles::tuning`).

A hand holding anything of the loadout table's takes no stick, lever, or button, as for the wheel ([steering-wheel.md](steering-wheel.md)).

## The sticks

A stick is grabbed like every physical control: a grip press with the hand's grip within `PHYSICAL.grab_radius_m` of the stick's top takes it, and releasing the grip, or moving further than `PHYSICAL.release_distance_m` from it, lets go. The grab is relative. The hand's offset from the top is taken when it grabs, wherever the top is at that moment, so a hand that takes the stick off-centre, or takes it again while it is still springing back, moves it only by moving. The holding hand is drawn on the top at that offset.

The top follows the hand across the horizontal plane of the cockpit frame, up to `max_deflection_m` from centre (0.10 m for the flight stick, 0.12 m for the cyclic). The output is the deflection's share of the full deflection past `deadzone` (5%), rescaled so that it still reaches one, so the tremor of a resting hand flies nothing. Pushing forward pitches the plane's nose down and the helicopter forward, and pushing right rolls right.

Let go, a centring spring returns the stick: its deflection decays with the time constant `return_s` (0.12 s), and the stick keeps writing its value on the way back, so the plane comes back with it rather than snapping level. A thumbstick pushed out of its deadzone while the stick returns snaps the stick to centre, so the thumbstick flies at once. Before the spring, a released stick snapped to centre on the tick it was let go.

### The rudder

`physical.twist_rudder` (on) works the plane's rudder by the held flight stick's twist about its shaft, the cockpit frame's vertical: turning the top to the right yaws right. The twist is measured from the hand's orientation when it took the stick, so a wrist held at any angle starts from no rudder. Twists within 6° work nothing, and 35° gives a full rudder (`PHYSICAL.flight`). Tilting the wrist about the other axes does not count. While the stick is held with the twist on, it replaces the tables' rudder writes, as it replaces their pitch and roll; let go, the stick's rudder stops and the thumbstick's rudder stands. The cyclic has no twist; the helicopter's yaw stays on the thumbstick.

## The levers

A lever's handle follows the hand along its axis, the collective's up and the throttle's forward, and its position over half its travel is the lever's value, `-1..=1`, with `detent` (10%) of the travel around neutral giving nothing. The collective returns to neutral when it is let go; the throttle stays where it is let go and keeps writing its value. A trigger pressed while the throttle is parked flies the plane for as long as it is pressed, and the throttle takes over again, where it was, once the trigger is let go. Before, a parked throttle overrode the trigger.

### The afterburner gate

The plane has no nitrous input of its own: its input reader ([`AirPlane::UpdateVehicleInputsFromActionMap`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane##func-UpdateVehicleInputsFromActionMap)) feeds the nitrous whenever the thrust input is above [`AirPlane::NITROUS_THRUST_THRESHOLD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane) ([input.md, "The plane's thrust and nitrous"](../../engine/gameplay/input.md#the-planes-thrust-and-nitrous)). A throttle that ran straight to full would light the afterburner in the last stretch of every push. The throttle's forward half has a gate starting at 85% of its travel (`PHYSICAL.flight.gate_at`). Below the gate the thrust rises from the detent to just under the game's threshold; in the gate it is full, and the afterburner burns where the game allows it (the nitrous upgrade). Without the upgrade, the gate is simply full thrust. Pulled back past idle, the throttle brakes, as the gamepad's left trigger does.

## Haptic cues

The held sticks and levers give their hand a cue of the vehicles category ([haptics.md](haptics.md)), each firing once as the control reaches it and re-armed only once the control has left it by a tenth of its travel, so a hand resting at a stop or wobbling about neutral does not buzz:

- a bump (`Event::ControlStop`) when a stick reaches its full deflection or a lever the end of its travel;
- a tick (`Event::ControlNeutral`) when a lever comes back to neutral or passes it: the throttle's idle and the collective's middle;
- a firm bump (`Event::ThrottleGate`) when the throttle goes into its gate and when it comes back out, with a little hysteresis on the way out.

One cue fires a tick. A lever that passes neutral on the tick it leaves the gate cues neutral on the next.

## Cockpit buttons

The horn and the nitrous are worked by hand (`buttons.rs`, with the fixed `PHYSICAL.buttons`). The nitrous is also on the dominant stick click; the horn is on no controller button by default (its `horn` action ships unbound), so the cockpit's horn is how it sounds; it also toggles a vehicle's magnet (`TOGGLE_MAGNET_1`), as the gamepad's left stick click does:

| Seat | Horn | Nitrous |
| --- | --- | --- |
| Car and boat, with the wheel | Push a free hand into the wheel's hub | Push a free hand into the button on the dash beside the wheel |
| Motorcycle, ATV, jetski, and duck, with the handlebars | The button | The button (the bars' grips are the throttle and the brake, below) |
| Helicopter, with the collective | The button | Twist the collective's grip either way |
| Plane | The button | Push the throttle through its afterburner gate (above) |

A push button is a disc facing the driver: the horn is the hub, 35% of the wheel's radius across, and the nitrous button sits 12 cm right and 2 cm up from the wheel's right edge, 4.5 cm across, facing the driver as the wheel does. Both ride the wheel as it is fitted to the vehicle ([steering-wheel.md](steering-wheel.md)). A hand arms a button by coming to it from the front within its radius, and presses it by pushing on past its face by 2 cm; the button stays pressed, giving a centimetre before it lets go, until the hand comes back out or slides off its edge. A hand that reaches it from behind, as one through the dash does, or from the side, as one sweeping across the wheel does, presses nothing, and a hand holding a control presses nothing either, so the hands on the rim cannot sound the horn. The nitrous button is drawn faintly on the dash while the overlay is on, since the vehicle's model has none; both buttons are ringed while a hand is in front of them and lit while pressed.

The handlebars' right grip is the throttle, as a motorcycle's is, and the left grip is the brake, twisted the same way (`physical.bars_twist`, on): rolling a grip's top forward, away from the rider (clockwise seen from the bars' right end), about the line across the bars, from the orientation the hand took it in, works it from nothing within 4° to full at 30°. The throttle reaches the game as `ACCELERATE` and the brake as `REVERSE`, each alongside its trigger, the larger of the two winning, so the triggers still work with the hands on the grips; rolling a grip back closes it. `REVERSE` is what the left trigger writes, so the brake grip brakes a moving bike and, held once it has stopped, reverses it, as the trigger does. A grip clicks in the hand as it opens and ticks as it closes. Neither burns the nitrous: the nitrous is the dominant stick's click, as on every land vehicle. The collective's grip burns the nitrous while the hand holding it has twisted it 40° (well past the roll a wrist makes while it raises the lever) either way about the fore-aft axis, and stops once the twist comes back by 8°. The handlebars' and the collective's own motion is untouched by either: the bars turn by the grips' positions, and the collective moves along its axis.

A pressed horn holds `SOUND_HORN_SIREN` and `TOGGLE_MAGNET_1`, and the nitrous holds `USE_VEHICLE_MOD`, which the game reads as the nitrous while it is held and as the turbo jump when it is tapped ([input.md](../../engine/gameplay/input.md#land-vehicle-generic-car-and-motorcycle)), so a short press of the nitrous button is a turbo jump, as a tap of the gamepad's button is. Both need the vehicle's upgrades as the gamepad does. Each press and twist clicks in its hand, and each release ticks. A hand pressing a button takes the flat palm's finger pose ([grip-poses.md](grip-poses.md)); a twisting hand keeps the pose of the control it holds.

### The landing gear

The game has no input for the landing gear. While the local player drives, [`Vehicle::UpdateLandingGearVariables`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-UpdateLandingGearVariables) chooses the gear's state itself on every update ([vehicle-parts.md, "Landing gear"](../../engine/gameplay/vehicle-parts.md#landing-gear)), so a gear lever would have nothing to drive. Overriding the choice would mean taking over that update for the player's plane; it is not built.

## Settings

The switches are `controllers.vehicles.physical.sticks` (the cyclic and the flight stick), `physical.levers` (the collective with its nitrous twist, and the throttle with its afterburner gate), `physical.nitrous_button`, `physical.bars_twist` (the bars' throttle and brake grips), and `physical.twist_rudder`; the horn comes with the wheel (`physical.wheel.enabled`). They are in the debug UI's Vehicles feature, under Movement/vehicles. Everything else is fixed in `PhysicalControlsTuning`: per stick the top, the full deflection, the deadzone, and the centring time; per lever its neutral, axis, travel, and detent; the rudder twist's deadzone and full twist and where the gate starts; and the hub's share, the nitrous button's place and size, the press depth, the collective's twist, and the bars' grips' full twist.

## What is logged and shown

The input tick logs `the throttle went through its afterburner gate` and `the throttle came back out of its afterburner gate` at info level. The debug UI's Status page has a FLIGHT CONTROLS row in its VEHICLES section: `WAITING, NOT FLYING A PLANE` outside a plane's driver's seat, and in it whether the stick is held, returning, or free, the rudder, the throttle with `AFTERBURNER` while it is through the gate (or `THROTTLE BY TRIGGER` while the trigger has it), and the counts of cues and burns. The Vehicles feature's live status opens with the VEHICLES section's rows, and adds the written rudder and the last cue beneath them. The recorder's signals are `vehicles.control_cues`, `vehicles.afterburner`, and `vehicles.afterburner_lights`, and `testing/scenarios/flight_controls.toml` is the template.

Each press of a button and each twist that opens a grip logs `the hand worked the horn`, `the hand worked the nitrous button`, `the hand worked the handlebars' throttle grip`, `the hand worked the handlebars' brake grip`, or `the hand worked the collective's twist grip` at info level, with the hand. The VEHICLES section's COCKPIT BUTTONS row names what the seat has (`HORN, NITROUS BUTTON` with the wheel, `THROTTLE AND BRAKE GRIPS` with the handlebars, `TWIST GRIP` with the collective, `WAITING, NONE IN THIS SEAT` in a plane), whether the horn or the nitrous is on, and the count of presses; out of the driver's seat it reads `WAITING, NOT DRIVING`. The recorder's signals are `vehicles.button_presses`, `vehicles.horn`, and `vehicles.nitrous`, and `testing/scenarios/cockpit_buttons.toml` is the template.

## Unverified

- That the plane's afterburner lights through the gate with the nitrous upgrade, and that the dry throttle's top, just under the threshold, does not light it.
- That the rudder's sign is right: the top turned right yaws the plane right.
- The flight stick's throw, the centring time, and the twist's range against the game's own response.
- Whether the throttle's brake half (pulled back past idle) is the brake the plane wants on the runway.
- That a hand resting near the hub does not sound the horn while the driving animation sways the fitted wheel, and the hub's share against the drawn hubs.
- The nitrous button's place beside the wheel, which has no drawn button to match, in cars with the wheel on the right.
- The twist grips' engaging twist, and that twisting a bars' grip does not turn the bars.

## Headset checklist

1. In a plane's driver's seat, take the throttle and push it forward: the thrust rises, and near the front a firm bump marks the gate. Through it, with the nitrous upgrade, the afterburner lights; the log has `the throttle went through its afterburner gate`. Pull back out: a click, and the afterburner goes out. Push on to the stop: a bump.
2. Let go of the throttle at half: the plane keeps that thrust. Press the right trigger: the trigger flies the plane; let go of the trigger and the throttle's thrust comes back. Pull the throttle back through idle into the brake: a light click at idle.
3. Take the stick off-centre and fly: no jump at the grab; a small tremor does not move the plane; full deflection gives a bump. Let go mid-roll: the stick and the plane come back level over a fraction of a second. Take the stick again while it returns: no jump.
4. Twist the held stick to the right: the plane yaws right, and the FLIGHT CONTROLS status row shows a positive rudder. With `flight.twist_rudder` off, twisting does nothing and the thumbstick's rudder works.
5. In a helicopter: the cyclic springs back the same way, and the collective clicks at its middle and bumps at its ends.
6. In a car, hold the wheel with one hand and push the other into the hub: the horn sounds with a click, and stops as the hand comes back out. Rest a hand on the hub without pushing, and sweep a hand across the wheel to take the far rim: no horn. With both hands on the rim, no horn.
7. With the nitrous upgrade, find the faint button right of the wheel and push it: the nitrous burns while it is held, and a short press is a turbo jump. The COCKPIT BUTTONS status row shows `NITROUS ON` while it is pressed.
8. On a motorcycle and on a jetski, twist the right grip forward (its top rolling away from you) while riding: the throttle opens from a few degrees to full at about 30°, a click marks it, no nitrous burns, and twisting back does nothing. Twist the left grip forward the same way: the bike brakes, and held at a stop it reverses. In a helicopter, twist the collective's grip either way: the nitrous burns.
9. In a plane, check that the gear still comes down and goes up by itself.
