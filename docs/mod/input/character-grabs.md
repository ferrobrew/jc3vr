# Taking hold of characters

An empty hand can take hold of a character. A character seated in a vehicle is reached through an open door: grip them and yank, and the player pulls them out of the seat and takes it, the game's own hijack. The code is `payload/src/controllers/physical_hands/characters/`, on top of the pickup ([physical-hands.md](physical-hands.md#picking-things-up)) and the doors ([vehicle-doors.md](vehicle-doors.md)); the engine side is in [vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#entering-through-a-door). It is on by default (`physical_hands.characters.hijack`, `PULL OCCUPANTS OUT BY HAND` on the pause menu's `VR: CONTROLS` page). It has not yet been seen working in the headset.

## Reaching a seated character

The pickup's reach rays find characters as well as objects: a hit on a character carries the character and the body hit, its capsule or a ragdoll body. A character attached to a vehicle ([`Character::IsAttachedToVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToVehicle)) is a target only when one of that vehicle's doors stands open at least `characters.hijack_min_open_deg` ([`PfxBreakable::GetHingeAngleAboveMin`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PfxBreakable##func-GetHingeAngleAboveMin)) with its box's centre within `characters.hijack_door_reach_m` of the palm: the hand reaches in through the door, not through the glass. A gun in the character's hands is the weapons' own target and wins over the character ([physical-weapons.md](physical-weapons.md)). With `pickup.show_targets`, a reached character is marked with a violet ring.

## Holding on and yanking

A grip on a character holds the hand at the grab point, carried with the character's own frame, the same way a latched door holds it ([vehicle-doors.md](vehicle-doors.md#the-latch)): a driven hand's body is held there, a probed hand drawn there. Pulling the controller `characters.yank_m` away from the grab point, in any direction, is a yank. The grip ends with it.

A yank on a seated character, through a door that is still open beside the hand, has the next input tick click the vehicle-entry action ([`ENTER_VEHICLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-ENTER_VEHICLE)), as the use button does on foot. The game then takes the player into the vehicle by the entry nearest the player, and an occupied seat is hijacked: the occupant is thrown out by the game's own animation. A yank after the door has swung shut does nothing but let go.

## Getting in

With a door open, the use button enters the vehicle as it always does, by the entry nearest the player, so the side whose door the hand opened is the side the player gets in by. The entry plays the game's own door animation; a hand still holding that door lets go when the player sits down, since the hands stand down in a vehicle.

## When the grip lets go

A grip on a character lets go with the grip, with a yank, when the hand stands down, and when the character's body no longer belongs to the character (gone or replaced), which logs nothing beyond the pickup's lost count and error.

## Settings and readouts

The settings are `physical_hands.characters`, under the doors' in the debug UI's Controllers tab: the hijack's switch, the door's least opening and reach, and the yank's distance. The readout says when a hand reaches a seated or a standing character and behind which door, and counts the grips and the occupants pulled out. The `VR: STATUS` page's `CHARACTERS` row, under `HANDS IN THE WORLD`, shows the same counts. The recorder's signals are `characters.grips` and `characters.hijacks`, and the template is `testing/scenarios/hijack.toml`.

The log has, at info level, `physical hands: characters: a hand took hold of a character` with whether they are seated, `physical hands: hijack: a hand pulled an occupant out through the open door` with the door, its opening, and the yank's direction, and `the door beside the hand closed before the yank`.

## Unverified in the headset

- That the reach rays hit a seated character through an open door, and that a character in a vehicle reports itself attached to it.
- That a clicked vehicle entry by the open door hijacks the driver's seat, rather than entering a passenger seat or doing nothing, and how it behaves with the player's hand still holding the door.
- How a hand body held at a point inside the cabin behaves against the seats and the vehicle's body.
- That the use button's entry by an open door picks that door's seat.

## Headset checklist

With `PULL OCCUPANTS OUT BY HAND` and `OPEN DOORS BY HAND` on and `pickup.show_targets` on in the debug UI:

1. Beside a car stopped with a driver, open the driver's door by hand ([vehicle-doors.md](vehicle-doors.md#headset-checklist)) past the hijack's opening.
2. Reach in toward the driver's shoulder: a violet ring, and the readout says the character sits behind the front door. With the door shut, no ring.
3. Grip: `a hand took hold of a character`, and the rendered hand stays on the driver.
4. Yank back: `a hand pulled an occupant out through the open door`, the game's hijack plays, and the player ends in the driver's seat.
5. Open a door of an empty car by hand and press the use button: the player gets in on that side.
