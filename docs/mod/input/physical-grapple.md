# Physical grapple

The grapple hand works the line: a pull back toward the body starts and speeds the reel, a yank during the reel boosts it, pulling back with tethers attached tightens them, the arm's motion pumps the swing while hanging, and the hand feels the line's tension. Every part sits on top of the existing bindings, so the buttons keep working and the hand adds to them. The implementation is `payload/src/grapple/physical/`, with the hang swing in `payload/src/controllers/hands/anchor.rs`; the engine side is in [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md), "The reel's speed and the tether squeeze".

## The grapple button

The grapple is the off-hand's upper face button (Y on the left Touch controller, B on the left Index controller), not the grip, which takes what the hand reaches ([grip.md](grip.md)). With nothing attached the button is the gamepad's grapple button, passed to the game as it is pressed: a tap fires (a zip, or a grab of what the hook hits), and a hold on a target, then aiming at a second and letting go, tethers the two. With tethers or a line attached, the bindings read the button's gestures before the game sees it (`payload/src/controllers/bindings/grapple_button.rs`):

| Press | Effect |
|---|---|
| A tap (released within 0.25 s) | Fires the grapple, once no second tap follows within 0.3 s; on a line with no tethers, at once |
| A double tap | Releases every tether: the gamepad's B (`RELEASE_GRAPPLE`, `CANCEL`, `BYPASS_FALL_PREVENTION`) on foot and in the wingsuit, the release alone in a vehicle. Nothing fires |
| A hold with a grapple target under the aim | The game's hold, a new tether |
| A hold with nothing new under the aim, tethers attached | Tightens the tethers: the retract rises from 0.35 to 0.7 over a second, and pulling the hand back while holding tightens harder (the larger of the two wins) |
| A sharp pull of the hand while the button is held, a line attached | Zips along the line, as a tap would |

The pulls act only while the button is held: a hand moved back in the course of play never zips or tightens. The L trigger still retracts the tethers on its own while the left hand holds no gun.

## What the hand does

| Situation | Hand | Effect |
|---|---|---|
| A line is attached, nothing carries the character, and the grapple button is held | A sharp pull back toward the body (the yank) | The grapple's tap, which zips |
| Reeling in | The pull's speed | The reel's speed and acceleration scale from `reel.min_scale` to `reel.max_scale` |
| Reeling in | A yank | The reel takes the game's retract boost for `reel.boost_s` |
| Tethers attached, on foot or falling, no line of the grapple's own, the grapple button held | Pulling back from where the hand reached | The game's retract input, which tightens the tethers |
| Hanging from the line | Holding the hand off where it took the line, and moving it | The body swings against the hand: the hold, plus a pump while the hand moves |

Pointing at the first object and firing, then at the second and firing, is the grapple's hand aim with the grapple button's held `FIRE_GRAPPLE` ([controllers-and-roomscale.md](controllers-and-roomscale.md#the-grapple)); the second fire comes from the same hand.

## The pull

The grapple hand's distance from the body point the yank uses (the head, lowered by `bindings.yank.body_drop_m`, in the cockpit frame) is sampled every input tick. The pull tracker (`grapple::physical::pull`) reads two things from it:

- the pull's speed: how fast the hand comes toward the body, peak-held and decaying with `pull.speed_hold_s` as its time constant, so that a pull at the start of a reel still counts through a reel longer than the arm's reach;
- the pull's depth: how far the hand is back from a reference that jumps out with the hand and creeps toward a hand held still at `pull.relax_mps`. Reaching out and pulling back deepens it, holding the hand back lets it fade, and a hand-over-hand pull keeps it up.

An untracked hand forgets the pull.

## The reel

While the mode detector sees the character in the reel's wind-up or reeling in, the input tick publishes a drive: the scale from the pull's speed (`reel.min_scale` with no pull, rising linearly to `reel.max_scale` at `reel.full_speed_mps`) and whether a boost runs. A detour on the reel's velocity update ([`NStateTask_ReelIn::UpdateReelVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-UpdateReelVelocity)) hands the local player's update a copy of the [`GrappleTuning`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrappleTuning) with the top speeds and accelerations scaled; the shared tuning resource is never written, and other characters' reels are untouched. The copy covers the bound reel fields, which are all the update reads of the tuning. A drive older than a quarter of a second is ignored, so the reel falls back to the game's own as soon as the input tick stops publishing. With the default `reel.min_scale` of one, a reel the hand does not pull runs at the game's speed, and pulling only makes it faster.

A second yank detector, with the bindings' yank thresholds, is armed only during the reel. A yank there runs the boost for `reel.boost_s`: the copy takes the tuning's retract top speed and acceleration in place of the plain ones, which is what holding the retract does for the game's boost. With `reel.boost_needs_upgrade` (the default) it applies only when [`GrapplingHook::GetRetractAcceleratesReelIn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetRetractAcceleratesReelIn) says the game's boost is unlocked. The boost does not touch the retract input, so it does not squeeze tethers. A long, fast pull that starts a zip can count as a boost too once the reel's wind-up arms the detector.

## The tether retract

The retract arms while the grapple button is held and the hook holds dual tethers ([`GrapplingHook::GetNumDualTethers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetNumDualTethers)), the grapple has no line of its own, nothing carries the character along a line, and the character is on the ground or falling. A line of the grapple's own is left out because the retract input pulls a light object on it toward the player, and a line being placed belongs to a tether not formed yet. The wingsuit and the parachute are left out because the retract input boosts them, and their arm steering and handles use the hands. On the tick it arms, the pull's depth starts from the hand where it is.

While armed, the pull's depth is turned into the retract value: nothing inside `tether.deadzone_m`, then the cube root of the pull's fraction of the span to `tether.full_m`. The game squeezes by the cube of the retract value, so the squeeze follows the pull's depth in proportion; its own ramp over the hold time still applies. The value goes into the bindings' writes as an analog `RETRACT_GRAPPLE`, merged with the trigger's, the larger winning.

## The hang swing

The hang task's detour already pushes a hanging body against the grapple hand's displacement from where it took the line (`hands.anchor.hang_swing`, the hold). The pump adds a push against the hand's velocity relative to the head, smoothed over `swing.smoothing_s`: zero below `swing.deadzone_mps`, rising to `swing.scale` times the game's own full-stick swing impulse ([`GrapplingHangTask_SmallImpulseSwingMagnitude`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHangTask_SmallImpulseSwingMagnitude)) at `swing.full_mps`. Pulling the hand back as the body swings forward and reaching out as it swings back feeds the swing. The hold and the pump together are capped at `swing.max_scale` times the game's impulse per hang tick, and each goes to the hang body at the hips as before. Either can be switched off on its own.

## Haptics

The fire, the attach (the snap as the hook takes hold), and the yank play their cues on the grapple hand (`Event::GrappleFire`, `Event::GrappleAttach`, and `Event::Yank`, [haptics.md](haptics.md)). On top, every input tick sends the grapple hand two levels: `Sustain::GrappleTension`, the tethers' highest tension ([`GrapplingHook::GetHighestWireTension`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetHighestWireTension)) while the game squeezes the tethers ([`GrapplingHook::m_IsSqueezingDualTethers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##field-m_IsSqueezingDualTethers)), and `Sustain::GrappleReel`, the reel's speed over its top speed while the hand drives the reel, each zero otherwise; the mixer sums them. A retract the pull starts plays `Event::TetherRetract` once. The game's own rumble for the squeeze and the reel still reaches the hands through the forwarded motors.

## Threading

The input tick (the game thread, outside the world update) reads the hook, steps the pull, publishes the drive, and sends the pulses after releasing its own lock. The reel's detour runs in the character's update on the game thread and reads the drive from a leaf lock. The readout is published to its own leaf lock, so the `GRAPPLE` status rows never wait on the input tick ([threading.md](../threading.md)).

## Settings and readouts

The switches are in the debug UI's Controllers tab: `bindings.yank.enabled`, `grapple.reel.enabled` and `grapple.reel.max_scale`, `grapple.tether.enabled`, and `grapple.swing.pump` under "Bindings", and `hands.anchor.hang_swing` under "Hands"; the line's vibration is behind `haptics.categories.traversal` ([haptics.md](haptics.md)). Everything defaults on. The tuning is under "Physical grapple" in the debug UI's Controllers tab, beside the yank's, and the weapons readout shows the pump and a line for the physical grapple.

The debug UI's Status tab has a `GRAPPLE` section:

- `GRAPPLE STATE`: `INACTIVE`, `FIRING`, `IN FLIGHT`, `ATTACHED`, `REELING IN`, `HANGING`, `REELED IN`, or `TETHER LINE`, or `WAITING, NO HAND INPUT` while the input tick does not run.
- `ATTACHED TARGET`: `OBJECT` or `SURFACE` with the distance from the head to the attach point, or `NONE`.
- `REEL FROM HAND`: the scale while the hand drives a reel, `BOOSTED` during a boost, and the counts of reels and boosts.
- `TETHERS`: how many are attached, the retract value while the pull drives it, the tension while the game squeezes, and the count of pulls.
- `HANG SWING`: the hang ticks that swung, and how many the pump added to.
- `LINE HAPTICS`: the grapple hand's line level, the stronger of the tension's and the reel's.

The log has, at info level under the `grapple` target, `physical grapple: the line attaches` with the target kind and distance, `the hand drives the reel` with the scale and the pull, `a yank boosts the reel`, `the reel ends` with the peak scale, `the hand retracts the tethers` with the count and value, `the tether retract ends` with the peak value, and `the tether count changes`.

## Headset checklist

With the physical grapple's switches on (the defaults):

1. Grapple to a wall and let the line hang without pulling: `ATTACHED TARGET` reads `SURFACE` with a plausible distance, `the line attaches` is logged, and the attach pulse is felt.
2. Hold the grapple button and yank to zip, then keep the hand still: `the hand drives the reel` with a scale near the yank's speed, and the reel is at least as fast as the gamepad's zip. `REEL FROM HAND` shows the scale during the reel and the count after; `the reel ends` follows.
3. Zip again and pull hard and steadily through the reel: a higher peak scale in `the reel ends`, a faster zip, and a stronger hum in the grapple hand.
4. With the retract boost upgrade bought, yank again during a long reel: `a yank boosts the reel`, `BOOSTED` in the row, and a visible speed-up. Without the upgrade, nothing changes speed and the row does not say `BOOSTED`.
5. Tether two objects (point and hold the grapple button at the first, then at the second, and let go): `the tether count changes` to one and `TETHERS` reads `1 ATTACHED`. Aim at nothing and hold the button: the tethers tighten, harder over a second. Still holding, reach out and pull back slowly: `the hand retracts the tethers`, the objects draw together faster, the retract pulse and then a hum that grows with the tension. Let go of the button: the tightening stops. The L trigger still retracts on its own. Double tap the button: the tethers release, and nothing fires.
6. With tethers attached and the button up, walk around and pull the hands back sharply: no retract and no zip (watch for `the hand retracts the tethers`).
7. Grapple onto a ledge into a hang and swing the arm forward and back in rhythm: the body swings and builds up, `HANG SWING` counts pumped ticks. Then move the hand wildly: the swing stays bounded.
8. Turn each switch off in turn and repeat its step: the behaviour returns to the buttons alone.

## Not yet done

- The second tether end from the other hand: both fires come from the grapple hand's aim. Firing the second end from the gun hand needs the grapple slot to follow that hand for one fire and a free input on it.
- A slower deliberate pull starting the zip: the zip still needs the yank's thresholds.
- The pull does not reel in a light object on the grapple's own line (the game's grapple pull on the retract input); the yank zips instead.

## Unverified in the headset

- That the reel's velocity update reads nothing of the tuning beyond the bound reel fields in every path, including the vehicle entries and the hang approach.
- How the scaled reel feels at the defaults, and whether `reel.min_scale` below one (a reel that slows when not pulled) is better.
- That the retract input's analog value drives the squeeze from the bindings' write as it does from the trigger.
- How hard the pump should push, and whether the hand's velocity relative to the head rather than to the line is the right measure while the body itself swings.
- Whether a hand pulled hard during the zip's wind-up gives an unwanted boost.
