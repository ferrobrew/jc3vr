# The act arbiter

The local player's character acts are dropped, replaced, or queued in one place, `jc3vr_body::controllers::acts`, which holds the order the owners apply in as a table rather than leaving it to the order of hooks and branches.

The game has two act queues. [`Character::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-QueueAct) forwards to [`AnimatedModel::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##func-QueueAct), and engine code that holds the model queues on it directly: the hit reactions ([state-signals.md](../../engine/character/state-signals.md)), the wingsuit's barrel roll ([wingsuit-evade.md](../../engine/gameplay/wingsuit-evade.md)), and the aim acts ([aim-state.md](../../engine/gameplay/aim-state.md)). A detour on each queue asks the arbiter: `hooks::character::acts` calls `filter_character`, and `hooks::character::signals` calls `filter_model`, which turns away models that are not the local player's. An act queued on the character passes the character entry, then the model entry with whatever the first let through. The model entry also stamps hit reactions for the procedural body ([procedural-body.md](procedural-body.md)).

## Claims and precedence

Each owner answers with a claim: drop the act, replace it with another when the state machine takes that one ([`AnimatedModel::TryAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##func-TryAct)), or queue it. Filters are asked in table order and the first claim wins, so a later owner's counters see only acts the earlier ones let through.

| Owner | Entry | Claims |
| --- | --- | --- |
| Vehicle reverse | Character | Drops the reversing acts, in which Rico looks over his shoulder; that is the player's head's job. |
| Mental state | Character | The automatic holsters under the controller scheme ([hands-and-aiming.md](../input/hands-and-aiming.md#the-aim-state-pin-and-the-mental-state-cuts)). |
| Locomotion | Character | Drops turn-in-place and replaces the turning starts, stops, flicks, and stunt turns ([locomotion.md](locomotion.md#the-turning-animations)). |
| Wingsuit evade | Model | Drops the barrel roll while the evade is off. |
| Weapon reconcile | Submits | The clip abort before the primary moves ([two-handed-inventory.md](../input/two-handed-inventory.md)). |
| Slide start | Submits | The plain move act in place of the wind-up while sliding. |

A refused replacement falls back to what its claim says: on foot the original act is queued, and a stunt turn is dropped.

## The flush

An owner that queues its own act submits it and flushes. The flush is the only code that queues or pre-flights an act for the local player: it queues the submissions in table order through `Character::QueueAct` (so the character filters see them), skips a guarded act the state machine would refuse, and does nothing for any other character. Submissions are per thread. Acts the mod queues on other characters (a punched rider's ragdoll, a seized gunner) stay with their subsystems ([physical-hands.md](../input/physical-hands.md)). The table is tested row by row in `controllers::acts::tests`.
