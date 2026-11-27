# Slow motion

The game's slow motion slows the world's clock: [`Clock::m_UpdateSpeed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_UpdateSpeed) is the game seconds per real second, set by the director's time scale and by the game's own effects (the wingsuit near-miss, the death camera). The player's body is not slowed, so the controllers keep two clocks apart (`jc3vr_body::controllers::clock`): what measures the player's body (a swing's speed, a pull, a yank, a gesture's dwell, a grip's timing) runs in real time, what drives the world (a body's velocity, a cooldown on the world's reactions, the reel's boost, the wire's flight) runs in game time, and a `TimeScale` converts where they meet.

The bindings tick's step is the game's, since the device poll receives the update context's scaled delta ([`GameObjectUpdateContext::m_Dt`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameObjectUpdateContext##field-m_Dt)), as are the aim, locomotion, and physics steps and the world's velocities ([`Character::m_MeasuredWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_MeasuredWorldVelocity)). The controllers' poses and display times are real. The bindings tick converts its step once (`real_dt`) and hands it to everything that times the player.

The cases that need care:

- Yanks, the reel's pull, the grip's fist window, the long presses, the reload's motions, and one swing's hit window use the real step, because they are the player's arm: timed in game seconds, a gentle drift in slow motion would count as a yank.
- A blow's needed speed is judged per real second, because a hand body following the real hand moves `1 / scale` as fast through the world.
- The hand bodies' drive raises its top speeds by `1 / scale` and its strength by the square, as the carry does ([physical-hands.md](physical-hands.md#holding)), so a hand body keeps up with a real swing. The palms' lead uses game seconds, or it would overshoot by `1 / scale`.
- The body rig's step drive is paced per real second and handed to the game per game second, so the body keeps up with the player's steps ([rig.md, "Steps"](../body/rig.md#steps)).

## Throws

A release (`jc3vr_body::controllers::physics::release`, for every prop, gun, grenade, and charge) takes the hand's velocity per real second and the player's movement through the rig per game second (`ReleaseMotion::in_game_time`). The hand's part is converted to the same motion per game second, `1 / scale` as fast in the world's units, while the tracking-glitch cap (`release.max_hand_mps`) holds per real second. So in slow motion an item leaves the hand as fast as the hand moved, rather than stopping dead against it, and carries farther, since the player is fast in a slowed world.

Unverified in the headset: that the poll's step matches the clock's scale at every director speed, how a slowed throw's carry reads, and that the scaled hand strength does not fling what a fast swing strikes.
