# The wingsuit's opening

The wingsuit and the parachute open from the air through two acts, `ACT_TRIGGER_WINGSUIT` and `ACT_TRIGGER_PARACHUTE`, which the base graph takes in the jumps as well as the falls. The decoded graph (`tools/anim_graph`, [animation-graph.md](../character/animation-graph.md)) shows the transitions; which input action raises each act is not traced here and is unverified.

## Where the acts are taken

The jumps (`S_JUMP_0` and its turned variants, the stunt jumps, and the train jumps), the falls (`S_FALLING`, `S_FAST_FALLING`, `S_FALL_TO_FREEFALL`), the reel-in's drops into a fall, and the falls out of the parachute and the suit all take `ACT_TRIGGER_WINGSUIT` to `S_JC_WINGSUIT_OPEN_FROM_FALL`, and `S_FREE_FALLING` takes it to `S_JC_WINGSUIT_OPEN_FROM_FREEFALL`. `ACT_TRIGGER_PARACHUTE` goes to the matching parachute openings from the same states. So a character that walks off a ledge can open either while the game still plays its jump: the jump state runs until its clip finishes and only then hands over to `S_FALLING`.

The jumps and `S_FALLING` gate both acts on the same pair of conditions, beside the suit or the canopy being enabled:

| Condition | Class | Shipped parameters |
|---|---|---|
| `C_JUMP_OR_FALL_TO_IN_AIR_SPEED` | `CVerticalSpeedCondition` | a vertical speed below -0.1 m/s: descending |
| `C_JUMP_OR_FALL_TO_IN_AIR_HEIGHT` | `CFallDistanceCondition` | a fall distance of at least 4 m |

`CFallDistanceCondition` reads zero while the character has ground contact, and otherwise a distance from the character's ground probe results; which distance (to the ground below, or fallen so far) is unverified. A press while rising from a jump, or too near the ground, opens nothing, and the act is not held over: a later press is needed. `S_FAST_FALLING` and `S_FREE_FALLING` take the acts on the enabled check alone.

## The boost take-off from the ground

The boost wingsuit's take-off from the ground is a third act, `ACT_BOOST_FROM_GROUND`, taken from the idle, the moves, and the turns on foot (gated on the suit being enabled and the boost suit owned) to `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO`, the wind-up, and then `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND`, the launch. The movement task does not run in the wind-up, so nothing turns the character there; the launch leaves along the character's facing. The hold that raises the act takes about a second of the use button on foot; which action the game reads for it is unverified.
