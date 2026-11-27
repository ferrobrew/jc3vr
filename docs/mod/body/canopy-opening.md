# The canopy's opening out of a reel

A canopy opened during a grapple reel plays one of the game's two openings out of a reel, `S_JC_PARACHUTE_OPEN_FROM_REEL_IN_LONG` or `S_JC_PARACHUTE_OPEN_FROM_REEL_IN_SHORT` ([engine: parachute-locomotion.md, "The openings out of a reel"](../../engine/gameplay/parachute-locomotion.md#the-openings-out-of-a-reel)). Both roll Rico over into the harness. The player's body does none of it, and the view, hung from Rico's head through the airborne transitions, rolls with him and passes through his harness and backpack. The mod removes the roll rather than moving the camera around it.

The opening is skipped for the local player while the head is decoupled (`crates/jc3vr-body/src/hooks/animation/canopy.rs`). The body machine enters the opening, so the opening's state tasks run (the canopy object's own opening act among them), and the local animation controller's update (the `AnimationController::UpdateAnimations` detour) finds it there before it evaluates the pose and moves it on to `S_JC_PARACHUTE_IDLE` through [`AnimationRuleSystem::SetStateById`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::rule_system::AnimationRuleSystem##func-SetStateById). No frame of the opening's clip is evaluated. A detour on [`StateMachineInstance::SetStateInternal`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::rule_system::StateMachineInstance##func-SetStateInternal) catches the transitions that call it as they happen; the act transition that opens the canopy does not call it ([parachute-locomotion.md](../../engine/gameplay/parachute-locomotion.md)), which is why the animation update checks too. The canopy's idle then flies from Rico's reel speed with the conventional parachute physics, without the opening's animation-driven lift.

The skip has no switch. The debug UI's Air feature counts the skips in its live status (`reel openings skipped`). Each skip is logged at info level as `canopy: skipped the opening out of a reel into the canopy's idle` with the opening's name.

## Headset checklist

1. Firing the grapple at a point ahead and opening the canopy (the button or the ripcord) during the reel: Rico goes straight to hanging under the canopy, with no roll, the view never passes through his body, and the canopy itself unfurls. The log has the skip line with the long or the short opening.
2. The same from a short reel off the ground (the pre-reel's wind-up): the same, with the short opening named.
3. Opening the canopy from a plain fall: the game's opening from a fall plays.
