# The wingsuit's evade

The wingsuit's evade is a barrel roll. It has one act, [`ACT_BOOST_WINGSUIT_BARREL_ROLL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::symbol_table::EventIdSymbolTable), and no state bit or sub-state of the wingsuit task of its own.

## Starting it

The air physics ([`NAirMovement_UpdateAirPhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-NAirMovement_UpdateAirPhysics)) starts the roll itself. When the [`WINGSUIT_EVADE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-WINGSUIT_EVADE) action fires, it tests a character value against a threshold, believed to be the boost's charge (unverified). Short of it, the HUD shows the boost's failure, unless an enemy missile is locked on, and nothing else happens. Otherwise, if the move stick is out of its rest, the air physics queues the roll's act straight on the character's animated model ([`AnimatedModel::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##func-QueueAct)). It does not go through [`Character::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-QueueAct), so nothing that watches the character's own act queue sees it. The character's queue forwards to the animated model's, so the animated model's queue sees every act, this one included.

The game's own binding for the action is the weaponized wingsuit's evade chord ([input.md](input.md)).

## The roll's directions

Up, down, left, and right are not separate acts. They are segments of the clip the act plays (`SEG_IN_BARREL_ROLL_EVADE_MOVEMENT` and its `_LEFT`, `_RIGHT`, `_UP`, and `_DOWN` variants, then `SEG_IN_BARREL_ROLL_RECOVERY_MOVEMENT`), so the one act covers every evade.

## What moves

The clip rolls the body's pose a full turn. While it is inside its evade-movement segment, the air physics builds the body's orientation from a matrix the character holds and the velocity's heading, rather than from the steering alone. That the matrix is the animated reference bone, so that the clip carries the body sideways through the roll with the alignment to the velocity off, is unverified.
