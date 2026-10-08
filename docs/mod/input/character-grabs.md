# Taking hold of characters

An empty hand can take hold of a character. On foot, a grip holds on to them: a yank throws a character the player is hostile to off their feet, and the hand then holds the ragdoll they fall as, to drag it and throw it with the hand's strength; a character the player is not hostile to only staggers. Letting go of a character with the hand moving shoves them. A character seated in a vehicle is reached through an open door: grip them and yank, and the player pulls them out of the seat and takes it, the game's own hijack. The code is `payload/src/controllers/physical_hands/characters/` and the grips in `physical_hands/pickup/grips.rs`, on top of the pickup ([physical-hands.md](physical-hands.md#picking-things-up)) and the doors ([vehicle-doors.md](vehicle-doors.md)); the engine side is in [melee-and-damage.md](../../engine/gameplay/melee-and-damage.md#the-hit-reaction) and [vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#entering-through-a-door). Both are on by default (`physical_hands.characters.grab`, `GRAB CHARACTERS`, and `physical_hands.characters.hijack`, `PULL OCCUPANTS OUT BY HAND`, on the pause menu's `VR: CONTROLS` page). None of it has yet been seen working in the headset.

## Reaching a character

The pickup's reach rays find characters as well as objects: a hit on a character carries the character and the body hit. A gun in the character's hands is the weapons' own target and wins over the character ([physical-weapons.md](physical-weapons.md)). With `pickup.show_targets`, a reached character is marked with a violet ring. What a hit is depends on the character:

- A character on foot, hit on its proxy's capsule ([`PfxCharacterInstance::GetCharacterRigidBodyId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##func-GetCharacterRigidBodyId)), is held on to (below).
- A hit on any other body of the character is a body of its ragdoll: it is taken as a loose object, by the object that owns the body, when it is dynamic and light enough ([physical-hands.md](physical-hands.md#holding)), so a fallen character is dragged by the limb the hand took, through a joint with the hand bodies' joint grabs, and thrown by letting go while swinging.
- A character attached to a vehicle ([`Character::IsAttachedToVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToVehicle)) is a target only when one of that vehicle's doors stands open at least `characters.hijack_min_open_deg` ([`PfxBreakable::GetHingeAngleAboveMin`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PfxBreakable##func-GetHingeAngleAboveMin)) with its box's centre within `characters.hijack_door_reach_m` of the palm: the hand reaches in through the door, not through the glass.

Whether the player's faction is hostile to the character ([`GetRelation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/faction##func-GetRelation)) is judged at the reach, as the blows judge it ([physical-hands.md](physical-hands.md#hits)).

## Holding on and yanking

A grip on a character's capsule holds the hand at the grab point, carried with the character's own frame, the same way a latched door holds it ([vehicle-doors.md](vehicle-doors.md#the-latch)): a driven hand's body is held there, a probed hand drawn there. Pulling the controller `characters.yank_m` away from the grab point, in any direction, is a yank, and ends the grip.

A yank on a character on foot pushes them along the yank through their own hit reaction ([`Character::HitReaction`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-HitReaction)), as a blow of `characters.yank_strength` of the hand blows' knockdown (`bodies.hits.knockdown_impulse_ns`), calibrated against the character's own knockdown threshold as a blow is, and without damage. The default reaches past a knockdown, so a hostile character falls as a ragdoll pushed along the yank. A character the player is not hostile to is staggered at most, as by a blow, and the hand lets go. After a yank on a hostile character, the hand stays at the grab point for up to `characters.regrip_s`, following the falling body, and as soon as its reach finds a body of that character's ragdoll it takes hold of it (`a hand took hold of a fallen character's ragdoll`); if none comes within reach in time it lets go.

Letting go of a character on foot, without a yank, with the hand moving at least `characters.shove_min_speed_mps`, shoves them along the hand's motion: a blow without damage of an arm of `bodies.hits.blow_mass_kg` at the hand's speed, under the same rules.

The reactions are queued from the palms' tick and dealt on the next frame tick, on the game thread outside the game's update, where the blows are dealt ([physical-hands.md](physical-hands.md#threading)); the character is found again from the body the hand held, so a character freed since is never touched.

A yank on a seated character, through a door that is still open beside the hand, has the next input tick click the vehicle-entry action ([`ENTER_VEHICLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-ENTER_VEHICLE)), as the use button does on foot. The game then takes the player into the vehicle by the entry nearest the player, and an occupied seat is hijacked: the occupant is thrown out by the game's own animation. A yank after the door has swung shut does nothing but let go.

## Getting in

With a door open, the use button enters the vehicle as it always does, by the entry nearest the player, so the side whose door the hand opened is the side the player gets in by. The entry plays the game's own door animation; a hand still holding that door lets go when the player sits down, since the hands stand down in a vehicle.

## When the grip lets go

A grip on a character lets go with the grip, with a yank (unless it waits for the ragdoll), when the hand stands down, and when the character's capsule no longer belongs to the character (gone or replaced). A hold on a ragdoll's body lets go as any hold does.

## Settings and readouts

The settings are `physical_hands.characters`, under the doors' in the debug UI's Controllers tab: the grab's switch, the yank's strength, how long the hand looks for the ragdoll, the shove's least speed, the hijack's switch, the door's least opening and reach, and the yank's distance. The readout says when a hand reaches a seated or a standing character and behind which door, and counts the grips, the yanks, the ragdolls taken, and the occupants pulled out. The `VR: STATUS` page's `CHARACTERS` row, under `HANDS IN THE WORLD`, shows the grips, the yanks, the knockdowns of the yanks and shoves, the ragdolls dragged, and the occupants pulled out. The recorder's signals are `characters.grips`, `.yanks`, `.knockdowns`, `.regrips`, and `.hijacks`, and the templates are `testing/scenarios/character_grab.toml` and `testing/scenarios/hijack.toml`.

The log has, at info level, `physical hands: characters: a hand took hold of a character` with whether they are seated, `a hand yanked a character` with whether the player is hostile to them, `a hand threw a character` with whether it was a yank, the hostility, the share of a knockdown, and the reaction impulse given, `a hand took hold of a fallen character's ragdoll` with the body and whether it is joined, `no ragdoll came within the hand's reach after a yank`, `physical hands: hijack: a hand pulled an occupant out through the open door` with the door, its opening, and the yank's direction, and `the door beside the hand closed before the yank`.

## Unverified in the headset

- That the reach rays hit a character's capsule and its ragdoll's bodies, and that a ragdoll's bodies are owned by an object the hold can check them against; if not, `a hand took hold of a fallen character's ragdoll` never appears and the hand gives up after the regrip time.
- That a hand held at a standing character's grab point pushes against their capsule acceptably, and how the character's own capsule listener reacts to it.
- That a yank at the default strength knocks a soldier down and a civilian only staggers, and that a dragged ragdoll is light enough for one hand.
- That the reach rays hit a seated character through an open door, and that a character in a vehicle reports itself attached to it.
- That a clicked vehicle entry by the open door hijacks the driver's seat, rather than entering a passenger seat or doing nothing, and how it behaves with the player's hand still holding the door.
- How a hand body held at a point inside the cabin behaves against the seats and the vehicle's body.
- That the use button's entry by an open door picks that door's seat.

## Headset checklist

With `GRAB CHARACTERS`, `PULL OCCUPANTS OUT BY HAND`, and `OPEN DOORS BY HAND` on and `pickup.show_targets` on in the debug UI:

1. Reach toward a standing soldier's collar: a violet ring. Grip: `a hand took hold of a character`, and the rendered hand stays on the soldier as they move.
2. Yank back: `a hand yanked a character` with `hostile=true`, then `a hand threw a character` with a strength above one; the soldier falls toward the player, and `a hand took hold of a fallen character's ragdoll` follows. Drag the body, then let go while swinging: it flies on.
3. Grip a civilian and yank: they stagger, `hostile=false`, and the hand lets go.
4. Grip a soldier, push the hand forward fast, and let go: `a hand threw a character` with `yank=false`, and the soldier staggers or falls along the push.
5. Beside a car stopped with a driver, open the driver's door by hand ([vehicle-doors.md](vehicle-doors.md#headset-checklist)) past the hijack's opening. Reach in toward the driver's shoulder: a violet ring, and the readout says the character sits behind the front door. With the door shut, no ring.
6. Grip the driver, then yank back: `a hand pulled an occupant out through the open door`, the game's hijack plays, and the player ends in the driver's seat.
7. Open a door of an empty car by hand and press the use button: the player gets in on that side.
