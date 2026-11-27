# The act arbiter

The local player's character acts are decided in one place, `jc3vr_body::controllers::acts`. Several subsystems drop, replace, or queue the player's acts, and the arbiter holds the order they apply in as a table rather than leaving it to the order of hooks and branches.

## Entries

The game has two act queues. [`Character::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-QueueAct) forwards to [`AnimatedModel::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##func-QueueAct) on the character's model, so an act queued on the character reaches both. Engine code that holds the model queues on it directly: the hit reactions ([state-signals.md](../../engine/character/state-signals.md)), the air physics' barrel roll ([wingsuit-evade.md](../../engine/gameplay/wingsuit-evade.md)), and the aim state's aim acts ([aim-state.md](../../engine/gameplay/aim-state.md)).

A detour on each queue asks the arbiter. The character entry (`hooks::character::acts`) asks `filter_character` for the local player's acts, and the model entry (`hooks::character::signals`) asks `filter_model` for every model's, whose filters turn away a model that is not the local player's. An act queued on the character passes the character entry first, and then the model entry with the act the first one let through, the original or its replacement. The model entry also stamps the hit reactions for the procedural body after its filters ([procedural-body.md](procedural-body.md)).

## Claims and precedence

Each owner answers with a claim: the owner, the act, and what to do with it, which is to drop it, to replace it by another act when the animation state machine takes that one ([`AnimatedModel::TryAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##func-TryAct)), or to queue it. A filter's claim answers the one queue call it was asked about; a submitted act stands until the next flush. At an entry the filters are asked in table order, the first claim wins, and a later owner is not asked, so its counters and the locomotion watchdog see only acts the earlier owners let through.

| Owner | Role | What it claims |
| --- | --- | --- |
| Vehicle reverse | Character entry | `ACT_REVERSE` and `ACT_REVERSE_MOTORBIKE` are dropped. They drive the reversing states in which Rico turns to look over his shoulder, which is the player's own head's job. |
| Mental state | Character entry | The automatic holsters under the controller scheme ([hands-and-aiming.md](../input/hands-and-aiming.md#the-aim-state-pin-and-the-mental-state-cuts)). |
| Locomotion | Character entry | The turn-in-place acts are dropped, and the turning starts, stops, flicks, and stunt turns are replaced ([locomotion.md, "The turning animations"](locomotion.md#the-turning-animations)). |
| Wingsuit evade | Model entry | The barrel roll is dropped while the evade is switched off in the headset. |
| Weapon reconcile | Submits | The draw, holster, or switch clip abort before the primary moves ([two-handed-inventory.md](../input/two-handed-inventory.md)). |
| Slide start | Submits | The plain move act in place of the wind-up starts while sliding, guarded by the pre-flight. |

A replacement the state machine refuses becomes what its claim says: on foot the original act is queued, and a stunt turn is dropped. The replacement then reaches the model entry like any other act.

## The flush

An owner that queues an act of its own submits it and flushes. The flush is the only code that queues an act on, or pre-flights one for, the local player. It queues the submitted acts in table order through `Character::QueueAct`, so the character entry's filters see them as they see the game's, skips a guarded act the state machine would not take, and queues nothing on a character that is not the local player. Submissions are kept per thread, so a flush queues only what its own thread submitted. Both submitters flush at the point they submit: the slide start inside the game's `QueueStarts`, whose result depends on whether its act took, and the reconcile's abort just before it unequips the primary.

Acts the mod queues on other characters stay with their subsystems: a punched rider's ragdoll, and a seized gunner or a toppled character's ([physical-hands.md](../input/physical-hands.md)).

The table and the filters' gates are tested row by row in `controllers::acts::tests`.
