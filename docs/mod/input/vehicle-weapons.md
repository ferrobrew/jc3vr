# Weapons in vehicles

Out of the driver's seat the hands fire the weapons they hold, or the mounted gun they man. The driver's built-in vehicle weapons are in [controllers.md, "Vehicles, mounted guns, and flight"](controllers.md#vehicles-mounted-guns-and-flight). This covers shooting from a passenger or stunt seat by the hand's aim. The code is in `payload/src/controllers/vehicles/`: `passenger.rs`, with its call in the aim update (`controllers::aim`).

## Passenger and stunt seats

In a passenger seat, and on a stunt position, the weapon in the hands is the player's wielded weapon, so it already aims and fires by the hands as on foot: each gun aims along its holding hand's sight line through the per-weapon update, the character-level aim point follows the gun hand, and the trigger fires the passenger's fire action ([controllers.md, "Aiming"](controllers.md#aiming)). The seated body did not follow. Its upper body aims through the vehicle aim blend ([`VehicleAimBlendState::UpdateSamplers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::VehicleAimBlendState##func-UpdateSamplers)), which turns the torso and the gun arm toward the aim control's weapon point ([`PlayerAimControl::m_AimPos`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_AimPos), its weapon slot), and the game sets that point from the gaze. A passenger looking ahead while pointing out of the side window twisted toward the windscreen, and the shot left the drawn gun's muzzle at an angle to it.

With `vehicles.passenger_body_by_hand` (on), after the aim control's post-simulation update, in a vehicle seat other than the driver's and off a mounted gun, while a gun hand aims, the weapon slot of that point is set to the gun hand's aim point: the acting hand's, or the other hand's when only it holds a gun. The game copies the slot into the control parameters and the character's weapon point right after, in the post-camera update, so the blend, the weapon point, and everything that reads them follow the hand, and the body leans and points the gun where the hand does, as it does toward the gaze for the gamepad. The point is the same one the gun is handed, so the drawn gun and the shot agree. With no gun in the hands, or with the option off, the game's point stands. Off, the gun still aims by the hand; only the body follows the gaze.

## Settings, logs, and readouts

The pause menu's `VR: CONTROLS` page has `PASSENGER BODY AIMS BY HAND` under VEHICLES. The aim update logs `the seated body follows the gun hand's aim` when it starts and `the seated body stops following the gun hand's aim` when it stops, at info level. The `VR: STATUS` page's VEHICLES section has a PASSENGER AIM row: `FOLLOWING THE HAND` with the number of starts while it acts, `WAITING, NO GUN HAND AIMING` in a passenger seat with no gun aimed, and `WAITING, NOT A PASSENGER` elsewhere. The recorder's signals are `vehicles.passenger_aiming` and `vehicles.passenger_aim_starts`, and `testing/scenarios/passenger_aim.toml` is the template.

## Unverified

- That the vehicle aim blend turns the seated body toward the hand's point without a fight with the passenger's lean-out animation, and how far round it turns before the game's blend runs out of samplers.
- Whether a stunt position runs the same blend; on it the gun aims by the hand either way.
- Whether the game refuses a passenger's shot that points into the vehicle; if it does, the hand's point now decides that, rather than the gaze.

## Headset checklist

1. In a passenger seat of a car with a pistol drawn, look ahead and point the pistol out of the side window: Rico's torso and arm turn toward the side, the shots go where the hand points, and the log has `the seated body follows the gun hand's aim`. The VR: STATUS row reads `FOLLOWING THE HAND`.
2. Point ahead, behind, and up: the body follows as far as the game's blend goes, with no snapping when the hand crosses the vehicle's axis.
3. With `PASSENGER BODY AIMS BY HAND` off: the body turns toward the gaze again, and the shots still go where the hand points.
4. On a stunt position on a car's roof: the gun aims and fires by the hand.
5. Driving, and on a mounted gun: the row reads `NOT A PASSENGER`, and nothing changes.
