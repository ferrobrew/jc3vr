# Flight controls and cockpit buttons

The driver's seat has physical controls ([bindings.md, "Vehicles"](bindings.md#vehicles)): the plane's flight stick (or its yoke, where the cockpit models one) and throttle, the helicopter's cyclic and collective, and the cockpit's buttons. This page covers the sticks, the yoke, the levers, and the buttons; the wheel and the handlebars are in [steering-wheel.md](steering-wheel.md). The code is in `crates/jc3vr-vehicles/src/vehicles/driving/physical/` (`sticks.rs`, `yoke.rs`, `levers.rs`, `buttons.rs`, `cues.rs`, `native.rs`, and `calibration.rs`), with the pulses in `driving/haptics.rs`. The controls have no switches. Their placement and feel come from the `physical`, `controls`, `cues`, and `calibration` sections of the vehicles' [tuning file](../tuning.md) (`crates/jc3vr-vehicles/tuning/vehicles.toml`).

A hand holding a loadout item, a mounted gun, or another control takes no stick, lever, or button.

## Holding a control

A grip press with the hand's grip within `physical.grab_radius_m` of a control takes it, and only releasing the grip lets go: a holding hand that drifts off the control keeps working it from wherever it is. The grab is relative. The hand's offset from the control is taken when it grabs, so a hand that takes a control off-centre, or takes it again while it springs back, moves it only by moving. The holding hand is drawn on the control at that offset.

## The sticks

A stick's top follows the hand across the cockpit frame's horizontal plane, up to the stick's `max_deflection_m`. The output is the deflection's share of full deflection past `deadzone`, rescaled to still reach one, so a resting hand's tremor flies nothing. Pushing forward pitches the plane's nose down and the helicopter forward, and pushing right rolls right.

Let go, a centring spring returns the stick with the time constant `return_s`, and the stick keeps writing its value on the way back, so the aircraft comes back with it rather than snapping level. A thumbstick pushed past its deadzone while the stick returns snaps it to centre, so the thumbstick flies at once.

The held stick's twist about its shaft works the plane's rudder and the helicopter's yaw: turning the top right yaws right. The twist is measured from the hand's orientation at the grab, so a wrist held at any angle starts from no rudder, and only the twist about the shaft counts (`physical.flight.twist_deadzone_deg` to `twist_full_deg`). While the stick is held it replaces the binding tables' pitch, roll, and rudder writes; let go, the thumbstick's rudder stands again.

## The yoke

A plane whose cockpit models a control wheel offers the yoke (`ControlKind::Yoke`) in place of the stick; the Urga U-17 Akrobat (`privatepropeller`) is the one such plane. Either hand or both take it by its horns. It turns about its column for the roll (clockwise rolls right) and slides along the column for the pitch (pulled toward the pilot pitches up), within `physical.yoke`'s turn and travel and past its deadzone. Each hand's angle and place along the column are taken when it grabs, and with both hands on it the yoke takes the mean of what each would make it. Let go, it springs back as the stick does. The rudder stays on the thumbstick, since a yoke has no twist to give it.

## The levers

A lever's handle follows the hand along its axis: the collective up, the throttle forward. The collective's position over half its travel is its value in `-1..=1`, with `detent` around neutral giving nothing, and it returns to neutral when let go. The throttle stays where it is let go and keeps writing its value. A trigger pressed while the throttle is parked flies the plane while it is pressed, and the throttle takes over again, where it was, once the trigger is let go.

The throttle's travel is split by a gate at zero, its rest position. The back `physical.flight.reverse_share` is reverse and the rest is forward thrust. Pulled back to zero, the throttle stops there with a hard haptic stop; pulling on past zero by `zero_pull` takes it through the gate into reverse, which is proportional, and pushing forward by the same distance brings it back out. Within `zero_snap` of zero either way it gives nothing, and a throttle let go there snaps to zero, so a hand that means zero leaves no thrust on. Reverse is what the gamepad's left trigger writes, so on the ground it brakes and then backs the plane up.

The plane has no nitrous input of its own: its input reader ([`AirPlane::UpdateVehicleInputsFromActionMap`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane##func-UpdateVehicleInputsFromActionMap)) burns the nitrous whenever the thrust input is above [`AirPlane::NITROUS_THRUST_THRESHOLD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane) ([input.md, "The plane's thrust and nitrous"](../../engine/gameplay/input.md#the-planes-thrust-and-nitrous)). A throttle that ran straight to full would light the afterburner at the end of every push, so the forward section has an afterburner gate from `gate_at` up: below it the thrust rises to just under the threshold, and in it the thrust is full, which lights the afterburner where the nitrous upgrade allows.

## Where the aircraft controls sit

For the merged cockpits the table in `native::cockpit_asset` knows (the utility helicopter, the heavy troop helicopter, the fighter, the fighter-bomber, and the Akrobat), the controls are placed from grips measured in the cockpit part's mesh and carried by that part's live transform, so they ride the airframe. The pilot's seat, read from the seated character's eyes in the part's space, picks which seat's controls are offered. The heavy troop helicopter's controls are separate parts in the body's space: a cyclic for each seat and one collective by the right seat. A collective the mesh does not model (the utility helicopter's, and the heavy troop's left seat's) is placed from that seat's measured cyclic by `controls.collective_from_cyclic_m`.

Any other aircraft falls back to the fixed specs, placed in the cockpit frame relative to the seated character's eyes. That frame's yaw is re-based at the seat lock by where the head points, so fallback controls can land in a different place on each entry. The `driving position` log line names the cockpit's body part and gives the pilot's eyes in that part's space, which is what a new table entry needs.

A merged cockpit's controls can also be set from the hands. The debug UI's Air feature has "Set the cockpit controls from the hands": press it, close the panel, and sit at the controls, and a countdown (`calibration.countdown_ms`) ticks on both hands; it starts only at the controls, since the open panel holds the controllers, and opening the panel or otherwise standing down the controls restarts it. When it ends, the right hand's grip becomes the stick's (the cyclic, the flight stick, or the yoke's right horn) and the left hand's the lever's, in the cockpit part's space, for that part and seat, and both hands buzz. Each capture is written to `cockpit_calibration.toml` in the configuration directory, keyed by part and seat (`utility_body#0`), and loads on start, so its numbers can be read back into the table. "Clear this cockpit's calibration" returns the cockpit to the table. A request lapses after `calibration.expire_after_ms` away from any cockpit's controls.

## Haptic cues

The held sticks, the yoke, and the levers give their hand a cue of the vehicles category ([haptics.md](haptics.md)). Each fires once as the control reaches it and re-arms only once the control has left it by `cues.rearm` of its travel, so a hand resting at a stop or wobbling about centre does not buzz. A stop gives `ControlStop`; the collective's middle and a stick's or the yoke's centre on either axis give `ControlNeutral`; each `physical.flight.thrust_step` of the throttle's forward section gives `ThrottleStep`; the throttle's zero stop gives `ThrottleZero`, its passage through the zero gate `ThrottleReverse`, and its entry into or exit from the afterburner gate `ThrottleGate`. One cue fires per tick, in the order zero gate, zero stop, afterburner gate, stop, step.

## Cockpit buttons

The horn and the nitrous are also worked by hand, with `physical.buttons`. The nitrous is on the dominant stick's click too; the horn ships unbound on the controllers, so the cockpit's horn is how it sounds.

| Seat | Horn | Nitrous |
| --- | --- | --- |
| Car and boat, with the wheel | Push a free hand into the wheel's hub | Push a free hand into the button on the dash right of the wheel |
| Motorcycle, ATV, jetski, and duck, with the handlebars | The button | The button |
| Helicopter, with the collective | The button | Twist the collective's grip either way |
| Plane | The button | The throttle's afterburner gate |

A push button is a disc facing the driver: the horn is the hub (`hub_share` of the wheel's radius), and the nitrous button sits beside the wheel's right edge (`nitrous_from_rim_m`). Both ride the wheel as it is fitted to the vehicle. A hand arms a button by coming to it from the front within its radius and presses it by pushing past its face by `press_depth_m`; it stays pressed, with some give, until the hand comes back out or slides off its edge. A hand reaching it from behind, as one through the dash does, or from the side, as one sweeping across the wheel does, presses nothing, and a hand holding a control presses nothing, so hands on the rim cannot sound the horn.

The nitrous button exists only on a vehicle with nitrous, the ones whose HUD shows the nitrous meter: [`Vehicle::GetCurrentEffectiveNitrousUpgradeLevel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-GetCurrentEffectiveNitrousUpgradeLevel) is above none, read from the driven vehicle each input tick. The vehicle's model has no button, so the mod marks it once a hand comes in front of it, with a "Nitrous" label (`jc3vr_hud::hud::mark_labels`); both buttons are ringed while a hand is in front of them and lit while pressed. A vehicle with the turbo jump upgrade but not the nitrous one has no button and jumps from the stick's click.

The collective's grip burns the nitrous while its hand has twisted it `twist_deg` either way about the fore-aft axis, well past the roll a wrist makes raising the lever, and stops once the twist comes back by `twist_hysteresis_deg`. The twist does not move the collective. On the handlebars the triggers are the throttle and the brake, and rolling a grip works nothing.

A pressed horn holds `SOUND_HORN_SIREN` and `TOGGLE_MAGNET_1`, so it also works a vehicle's magnet, as the gamepad's left stick click does. The nitrous holds `USE_VEHICLE_MOD`, which the game reads as the nitrous while held and as the turbo jump when tapped ([input.md](../../engine/gameplay/input.md#land-vehicle-generic-car-and-motorcycle)), so a short press is a turbo jump. Each press and twist clicks in its hand and each release ticks. A pressing hand takes the flat palm's finger pose ([grip-poses.md](grip-poses.md)); a twisting hand keeps the pose of the control it holds.

The game has no landing gear input: while the local player drives, [`Vehicle::UpdateLandingGearVariables`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-UpdateLandingGearVariables) chooses the gear's state itself ([vehicle-parts.md, "Landing gear"](../../engine/gameplay/vehicle-parts.md#landing-gear)), so the cockpit has no gear lever.

## Diagnostics

The input tick logs `the throttle went through its afterburner gate` and `the throttle came back out of its afterburner gate`, each button press or engaging twist (`the hand worked the horn`, `the hand worked the nitrous button`, `the hand worked the collective's twist grip`), and `vehicle control: let go of the control` with its reason, all at info level. The Status page's VEHICLES section has a FLIGHT CONTROLS row (the stick's state, the rudder, and the throttle, in a plane's driver's seat) and a COCKPIT BUTTONS row (what the seat offers, what is on, and the press count). The Air feature's readout shows the calibration's state and the file's path.
