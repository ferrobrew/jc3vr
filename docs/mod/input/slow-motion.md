# Slow motion

The game's slow motion slows the world's clock: the clock's update speed ([`Clock::m_UpdateSpeed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_UpdateSpeed)) is the game seconds that pass per real second, below one in slow motion, set by the director's time scale and by the game's own effects (the wingsuit near-miss, the death camera). The player's body is not slowed, so the controllers keep two clocks apart, in `controllers::clock`:

- What measures the player's body runs in real time: a swing's speed, a pull, a yank, a gesture's dwell, a grip's timing, a fingertip's dwell on a button.
- What drives the game world runs in game time: a body's velocity, a cooldown on the world's reactions, a boost to the reel, the wire's flight.
- Where one meets the other, a `TimeScale` converts: a real rate is `1 / scale` as fast per game second, a game step lasts `1 / scale` as long in real seconds.

The clocks the controllers read are these. The bindings tick's step is the game's: the device poll receives the update context's scaled delta ([`GameObjectUpdateContext::m_Dt`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameObjectUpdateContext##field-m_Dt)), so in slow motion it is a fraction of the real frame. The aim update's step, the locomotion task's step, and the physics step are game time too, and the physics bodies' and characters' velocities ([`Character::m_MeasuredWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_MeasuredWorldVelocity)) are per game second. The controllers' poses, their display times, and the wall clock are real. The bindings tick converts its step once (`real_dt`) and hands it to everything that times the player.

## The rule applied

| What | Clock now | Why |
|---|---|---|
| The zip's yank, the snatch's yank, and the snatch's aim keeping a target (`bindings/yank.rs`, `snatch/`) | Real step | A yank is the player's arm: its speed, window, and cooldown were game seconds, so in slow motion a gentle drift counted as a yank. |
| The reel's pull and the boost's yank (`grapple/physical/`) | Real step | The pull's speed sets the reel's scale; it read four times high at a quarter. |
| The reel's boost | Game seconds | The boost speeds the reel, which runs in the world: it covered a quarter of its stretch of reel when timed in real seconds. |
| The grip's fist window and the long presses (`grab/latch.rs`, `bindings/tables/`) | Real step | The player's grip timing. |
| A blow's speed (`physics/hit.rs`) | Real, from the world's velocity | A hand body following the real hand moves `1 / scale` as fast through the world, so a reach looked like a swing. The speed a blow needs is judged per real second; the blow itself is the hand's speed through the world, as a throw is. |
| One swing, one hit (`physics/blows.rs`) | Real seconds | The player's swing; its contacts last as long in real time at any scale. |
| A character's damage cooldown | Game seconds | The world's pace of harm. It was also cut short by the per-hand gate's pruning, which shared its list; the two gates are separate now. |
| The hand bodies' drive (`physics/drive.rs`) | Scaled per tick | The top speeds rise by `1 / scale` and the strength by its square, as the carry's does ([physical-hands.md](physical-hands.md#holding)), so a hand body keeps up with a real swing. |
| The palms' velocity smoothing (`physics/observe.rs`, `physical_hands/tick.rs`) | Real stretch | The smoothing's time constant was game seconds, so throws and flicks were smoothed four times longer at a quarter. |
| The palms' lead (`physics/step.rs`) | Game seconds | The target's age, real, times a velocity per game second overshot by `1 / scale`. |
| The root's chase (`body/rig/runtime.rs`) | Real, converted | The chase closes the gap to the player's own stance: paced and capped per real second, then handed to the game per game second, so the body keeps up with the player's steps. |
| A throw (`hold::act::release_at`, every gun, grenade, and charge) | Real, converted | See below. |
| The reload: the taps, the flicks, the grip's motion, the round's insertion and its float into the well, the action's return, and the wrist detonator's dwell (`weapons/reload/`, `weapons/c4/wrist.rs`) | Real step | The player's hands and what they hold. A loose round carried as a body in the world keeps the game step. |
| The wingsuit gestures, the grip's hand speed, the haptics, the seated follow, the head fade, the weight spring, the dock slides, the recoil springs, the grapple hang's pump | Real already | Display times or the wall clock. The recoil's spring settles in real time while shots come further apart; it is the player's arm settling. |
| The snatch's wire timeouts | Game step | The wire flies in the world. |
| The ripcord | No clock | A distance from where it was taken. |

## Throws

A release takes the hand's velocity from the controller's history, per real second, and the player's movement through the rig, per game second (`ReleaseMotion::in_game_time`). The hand's part becomes the same motion per game second, `1 / scale` as fast in the world's units, and the cap against tracking glitches (`MAX_THROW_MPS`) holds per real second. In slow motion an item leaves the hand as fast as the hand moved, carrying on from the hold, which keeps a held body with the hand in real time, rather than slowing by the scale the moment the fingers open. The player is fast in a slowed world, so a throw also carries farther: at a quarter, four times as fast, and on the world's gravity farther still. Leaving the hand's real speed unconverted would keep a throw's arc the same as at full speed, but the item would stop dead against the hand that threw it.

## Unverified in the headset

- That the device poll's step is the scaled delta at every speed the director sets, and that the clock's update speed is the scale it was scaled by.
- That a throw in slow motion leaves the hand without a visible change of speed, and how far its carry reads in footage.
- That a hand body follows a fast swing in slow motion without the scaled strength flinging what it strikes.
