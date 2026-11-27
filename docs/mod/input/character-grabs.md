# Taking hold of characters

An empty hand can take hold of a character. A grip on a standing character throws them into their ragdoll and holds it by the bone nearest the palm, to be dragged and thrown; while held, the game's get-up is held off, and let go they get up the game's own way. A body on the floor, knocked down or dead, is gripped by the nearest limb. A seated character is reached through an open door, where a yank is the game's own hijack, or through a window the hands brought down ([breaking-glass.md](breaking-glass.md)), where a yank pulls them out as a ragdoll. The grapple's snatch swings a held body on its line ([snatch.md](../grapple/snatch.md#swinging-on-the-line)).

The code is `crates/jc3vr-physical/src/controllers/physical_hands/characters/` and `physical_hands/pickup/grips.rs`, on top of the pickup and holding ([physical-hands.md](physical-hands.md#picking-things-up)), tuned in `characters.toml`, with the `Characters` row in the debug UI's Characters and doors feature and its log under `physical hands: characters:`. The engine side is in [melee-and-damage.md](../../engine/gameplay/melee-and-damage.md#the-hit-reaction), [ragdoll.md](../../engine/character/ragdoll.md), and [vehicle-doors.md](../../engine/gameplay/vehicle-doors.md#entering-through-a-door).

## Reaching a character

The pickup's reach rays find characters too. A gun in or on the character is the weapons' target and wins whenever it is within reach ([physical-weapons.md](physical-weapons.md#taking-a-gun-from-a-character)). Otherwise:

- A protected character is never a target ([ragdoll.md](../../engine/character/ragdoll.md#protected-characters), `physics::characters::protection` in the body crate): one a mission marks critical ([`m_IsMissionCriticalCharacter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_IsMissionCriticalCharacter)), one in a scripted sequence ([`m_InSequence`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_InSequence)), and the player.
- A character on foot, hit on its capsule, is thrown into its ragdoll or held on to.
- A ragdolling character ([`Character::IsRagdolling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsRagdolling)) is taken by the ragdoll body the ray hit, or the one nearest the hit, since the ray may meet the capsule instead (`characters::ragdoll`). A body still in its death animation is not taken, nor a standing character's animation-driven ragdoll.
- A character attached to a vehicle ([`Character::IsAttachedToVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToVehicle)) is a target only beside an open door of that vehicle ([`PfxBreakable::GetHingeAngleAboveMin`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::parts::PfxBreakable##func-GetHingeAngleAboveMin)) or when the hand came in through a window of it the glass brought down. Intact glass is never a way in.

Faction relation ([`GetRelation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/faction##func-GetRelation)) is logged and decides nothing.

## Holding a ragdoll

The hand's joint carries only the gripped bone (`pickup::tick::joined_mass_kg`); the rest of the body's weight reaches it through the ragdoll's own constraints, so the body hangs limp from the grip. The whole ragdoll's mass still decides whether the hands lift it. The softer drive, point joints, and damping are in [physical-hands.md, "Holding through a joint"](physical-hands.md#holding-through-a-joint). A dragged character is hurt only as far as the game's own ragdoll impacts hurt it.

A grip on a standing, living character whose state allows it ([`SB_SKIP_HITREACTION_RAGDOLL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_SKIP_HITREACTION_RAGDOLL)) topples them (`characters::topples`) through their own [`Character::HitReaction`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-HitReaction), a damageless blow calibrated just past their knockdown threshold, along the hand's motion or toward the hand. The game's ragdoll, AI, and get-up handle it as any knockdown. The hand stays at the grab point and takes the nearest bone once its reach finds one. Some states make the hit reaction return without ragdolling (an ally holding her gun), so a character still standing after a moment has the ragdoll act queued directly, once (`physics::characters::retry_ragdoll`).

## Holding the get-up off

The graph stands a character up on its ragdolling timers ([ragdoll.md](../../engine/character/ragdoll.md#out-of-the-ragdoll)), and a held body counts as settled once its limbs move together, which would end a hold after a few seconds. So each pickup tick, once per held character, the hold writes the total ragdolling time in [`RagdollingControllerParams`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::control_parameters::RagdollingControllerParams) below zero (the value the ragdolling task keeps while the ragdoll's owner moves it, which the timer condition never holds on) and the settled time to zero (`characters::get_up`). Let go, the timers run again. A hold that the get-up ends anyway ([`SB_IS_GETTING_UP_FROM_RAGDOLL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_GETTING_UP_FROM_RAGDOLL)) logs a warning.

## Holding on and yanking

For a character whose state keeps it on its feet, a grip holds the hand at the grab point in the character's frame, as a latched door does ([vehicle-doors.md](vehicle-doors.md#the-latch)). Pulling the controller far enough away in any direction is a yank and ends the grip. A yank on a character on foot is a damageless blow past a knockdown along the yank, and the hand then waits briefly (in game time, so slow motion stretches it) to take the first ragdoll body its reach finds. Letting go of a standing character with a moving hand shoves them. Reactions are queued from the palms' tick and dealt on the frame tick ([physical-hands.md](physical-hands.md#threading)), the character found again from the held body.

A yank on a seated character through a door still open beside the hand has the next input tick click [`ENTER_VEHICLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-ENTER_VEHICLE): the game enters by the nearest entry and hijacks with its own animation. A yank after the door shut only lets go.

A mounted gun's gunner is not hijacked, since entry would seat the player elsewhere. A yank on a character in [`Character::IsInMountedGunState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInMountedGunState), in a gunner seat or on a tripod, queues [`ACT_FULL_RAGDOLL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::symbol_table::EventIdSymbolTable) once the seat accepts it (`physics::characters::gunner_out`), because the hit reaction never ragdolls a vehicle-attached character; the gunner falls out and the hand takes the ragdoll.

## Through a broken window

A shattered window is hidden, not removed, and glass never collides with the hands, so whether a hand came in through one is judged by casting along its way in (`characters::pane`): from the eyes to the palm and on to the occupant (`physical_hands::query::crosses_hidden_pane`), a brought-down pane of the occupant's vehicle before anything solid counting as a way in ([breaking-glass.md](breaking-glass.md#bullets-through-a-pane-brought-down)). The casts run only once a pane has been brought down and only for an enclosed occupant with no open door beside the hand: through a shattered windshield from the hood a hand reaches the driver, through the roof it does not.

A yank on such an occupant does not hijack; it queues the full ragdoll once the seat accepts it (`characters::unseat`, `physics::characters::seat_out`) and the hand drags them out through the opening. A punch through a broken window, like one on a motorcycle rider, queues the full ragdoll in place of a knockdown (`physics::blows::exposed_seat`), since the hit reaction never ragdolls a seated character. A protected occupant, or a seat that takes no full ragdoll, stays, and the hand lets go.

## Outstanding checks

- That a settled corpse's bodies wake when the hand moves them.
- That a clicked entry by an open door hijacks the driver's seat rather than a passenger's, with the hand still on the door.
- Which seat states besides a gunner's take the full ragdoll, and whether the ragdoll comes free of the cabin through the opening.
- That a vehicle's glass hits name the vehicle as their object, as the bullets' glass handling assumes.
