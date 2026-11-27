# Flight controls and cockpit buttons

The driver's seat has physical controls ([bindings.md, "Vehicles"](bindings.md#vehicles)): the plane's flight stick (or yoke) and throttle, the helicopter's cyclic and collective, and the cockpit's buttons. The wheel and handlebars are in [steering-wheel.md](steering-wheel.md). The code is `crates/jc3vr-vehicles/src/vehicles/driving/physical/`, with the pulses in `driving/haptics.rs`; placement and feel are the `physical`, `controls`, `cues`, and `calibration` sections of `crates/jc3vr-vehicles/tuning/vehicles.toml`.

## The sticks and levers

A grip press near a control takes it, and only releasing the grip lets go. The grab is relative, so taking a control off-centre or while it springs back never jumps it. A hand holding a loadout item or another control takes nothing.

The stick's top follows the hand across the cockpit's horizontal plane, past a deadzone so a resting hand flies nothing, and its twist about the shaft, measured from the wrist's angle at the grab, is the rudder. While held it replaces the binding tables' pitch, roll, and rudder. Let go, it springs back while still writing its value, so the aircraft comes back with it. A plane whose cockpit models a control wheel (only the Akrobat, `privatepropeller`) offers the yoke instead (`ControlKind::Yoke`): it turns for roll and slides along its column for pitch, and leaves the rudder on the thumbstick.

The collective has a detent at neutral and springs back; the throttle stays where it is let go. The throttle has a gate at zero: the back of its travel is reverse, which brakes and backs up on the ground, and a hand pulling back stops at zero with a hard haptic stop until it pulls further, so a hand that means zero leaves no thrust on. The plane has no nitrous input of its own: [`AirPlane::UpdateVehicleInputsFromActionMap`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane##func-UpdateVehicleInputsFromActionMap) burns nitrous whenever thrust exceeds [`AirPlane::NITROUS_THRUST_THRESHOLD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane) ([input.md, "The plane's thrust and nitrous"](../../engine/gameplay/input.md#the-planes-thrust-and-nitrous)). So below the throttle's afterburner gate thrust stops just under the threshold, and only in the gate is it full.

Each control gives its hand the vehicles category's cues ([haptics.md](haptics.md)) at its stops, centre, thrust steps, and gates, once each until the control moves away, so a hand resting on a stop does not buzz.

## Where the aircraft controls sit

For the cockpits in `native::cockpit_asset`'s table (the utility and heavy troop helicopters, the fighter, the fighter-bomber, and the Akrobat), the controls are placed from grips measured in the cockpit part's mesh and carried by that part's live transform, so they ride the airframe; the pilot's eyes in the part's space pick the seat. Other aircraft fall back to fixed specs relative to the seated character's eyes. The `driving position` log line gives what a new table entry needs.

The debug UI's Air feature can also set a cockpit's controls from the hands: after a countdown at the controls (with the panel closed, since the open panel holds the controllers), the right grip becomes the stick's and the left the lever's. Captures persist in `cockpit_calibration.toml` in the configuration directory, keyed by part and seat, so they can be read back into the table.

## Cockpit buttons

The horn and the nitrous can be worked by hand (`physical.buttons`). The nitrous is also on the dominant stick's click; the horn is on no controller button.

| Seat | Horn | Nitrous |
| --- | --- | --- |
| Car and boat | Push into the wheel's hub | Push the button right of the wheel |
| Handlebars | The button | The button |
| Helicopter | The button | Twist the collective's grip |
| Plane | The button | The throttle's afterburner gate |

A push button is a disc facing the driver that rides the wheel, pressed by a hand pushing past its face from the front. A hand from behind (through the dash), from the side (sweeping across the wheel), or holding a control presses nothing, so hands on the rim cannot sound the horn. The nitrous button exists only when [`Vehicle::GetCurrentEffectiveNitrousUpgradeLevel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-GetCurrentEffectiveNitrousUpgradeLevel) is above none, and the HUD labels it (`jc3vr_hud::hud::mark_labels`), since the model has none. The collective's twist grip burns nitrous only when twisted well past the roll a wrist makes raising the lever.

A pressed horn holds `SOUND_HORN_SIREN` and `TOGGLE_MAGNET_1`, so it also works a magnet. The nitrous holds `USE_VEHICLE_MOD`, which the game reads as nitrous while held and a turbo jump when tapped ([input.md](../../engine/gameplay/input.md#land-vehicle-generic-car-and-motorcycle)). There is no gear lever: [`Vehicle::UpdateLandingGearVariables`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-UpdateLandingGearVariables) chooses the gear's state itself ([vehicle-parts.md, "Landing gear"](../../engine/gameplay/vehicle-parts.md#landing-gear)).

The Status page's VEHICLES section has FLIGHT CONTROLS and COCKPIT BUTTONS rows, and the log records gate passages, presses, and let-goes.
