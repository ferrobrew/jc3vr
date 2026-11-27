# Physical grapple

The grapple hand works the line: a pull back along it starts and speeds the reel, a yank during the reel boosts it, pulling back with tethers attached tightens them, the arm's motion pumps the swing while hanging, and the hand feels the line's tension. Every part sits on top of the existing bindings, so the buttons keep working and the hand adds to them. The implementation is `grapple::physical` in `jc3vr-grapple`, with the hang swing in `jc3vr_body::controllers::hands::anchor`; the engine side is in [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md), "The reel's speed and the tether squeeze". The lock on the reel's line is in [grapple-lock.md](grapple-lock.md), the snatch in [grapple-snatch.md](grapple-snatch.md), and the grapple from a vehicle seat in [seat-grapple.md](seat-grapple.md).

None of it has a switch: it acts whenever the controller scheme is in effect. The tuning is the grapple's tuning file (`crates/jc3vr-grapple/tuning/grapple.toml`, [tuning.md](../tuning.md)): the `button` section for the button's gestures, `grapple_yank` and `yank` for the zip's yank, and `physical` (`PhysicalGrappleTuning`) for the pull, the reel, the retract, and the swing.

## The grapple button

The grapple is the off-hand's upper face button (Y on the left Touch controller, B on the left Index controller), not the grip, which takes what the hand reaches ([grip.md](grip.md)). With nothing attached the button is the gamepad's grapple button, passed to the game as it is pressed: a tap fires (a zip, or a grab of what the hook hits), and a hold on a target, then aiming at a second and letting go, tethers the two. With tethers or a line attached, the bindings read the button's gestures before the game sees it (`controllers::grapple_button`):

| Press | Effect |
|---|---|
| A tap (released within `button.tap_s`) | Fires the grapple, once no second tap follows within `button.double_tap_s`; on a line with no tethers, at once |
| A double tap | Releases every tether: the gamepad's B (`RELEASE_GRAPPLE`, `CANCEL`, `BYPASS_FALL_PREVENTION`) on foot and in the wingsuit, the release alone in a vehicle. Nothing fires |
| A hold with a grapple target under the aim | The game's hold, a new tether |
| A hold with nothing new under the aim, tethers attached | Tightens the tethers: the retract rises from `button.tighten_start` to `button.tighten_held` over `button.tighten_ramp_s`, and pulling the hand back while holding tightens harder ([The tether retract](#the-tether-retract); the larger of the two wins) |
| A sharp pull of the hand while the button is held, a line attached | Zips along the line, as a tap would |

During the reel and its wind-up, the button is the lock's ([grapple-lock.md](grapple-lock.md#the-button)). The pulls act only while the button is held: a hand moved back in the course of play never zips or tightens. The L trigger still retracts the tethers on its own while the left hand holds nothing that uses the trigger: no gun, and no grenade whose pin the press pulls.

## What the hand does

| Situation | Hand | Effect |
|---|---|---|
| A line is attached, nothing carries the character, and the grapple button is held | A sharp pull back along the hand's aim (the yank) | The grapple's tap, which zips |
| Reeling in | The pull's speed | Scales the reel's speed and acceleration ([The reel](#the-reel)) |
| Reeling in | A yank | The game's retract boost for `physical.reel.boost_s` |
| Reeling in, the grapple button held into the lock | Pulling back along the aim | Takes line in ([grapple-lock.md](grapple-lock.md#shortening-the-line)); a yank resumes the reel |
| Tethers attached, on foot or falling, no line of the grapple's own, the grapple button held | Pulling back from where the hand reached | The game's retract input, which tightens the tethers |
| Hanging from the line | Holding the hand off where it took the line, and moving it | The body swings against the hand: the hold, plus a pump while the hand moves |
| On foot, the grapple hand empty, no line out, the grapple button up, the grip pressed at a thing out of reach under the aim | A yank while the grip is held | The grapple snatch ([grapple-snatch.md](grapple-snatch.md)) |

Pointing at the first object and firing, then at the second and firing, is the grapple's hand aim with the grapple button's held `FIRE_GRAPPLE` ([hands-and-aiming.md](hands-and-aiming.md#grapple-aim)); both fires come from the grapple hand.

### The yank

The zip's yank is measured along the grapple hand's aim at the tick it armed, the first tracked tick with a line attached and the button held, in the cockpit frame (`controllers::yank::LineYank`): a drop of at least `grapple_yank.min_distance_m` in the grip's reach along that line, at `grapple_yank.min_speed_mps` or faster. A pull back from where the hand pointed counts whichever way it goes, so a seated player's hand, which starts close to the body and pulls back past its side, zips, though its distance from a point below the head hardly changes over such a pull.

A pull that carries the hand out from the head across the ground by more than `yank.outward_share` of the pull is not a yank, for the zip's and the reel's boost alike. Spreading the arms to open the wingsuit swings the grapple hand out to the side, which drops its reach along an aim pointed across the body as fast as a yank does, but carries it out from the head, where a yank brings the hand in, down, or back past the side. The distance is measured across the ground alone, so a yank down from a raised hand on a line to a high point, which takes the hand further from the head but not out from it, still counts. The line is taken afresh at each arming, and the hand's turn during the pull does not move it.

The latch is `controllers::yank::ArmedLine`, which every armed hand gesture on the line keeps: the reel's and the retract's pull, the lock's take-in, and the snatch's yank each measure along their own arming's aim the same way, though their detectors differ. The snatch's yank has its own thresholds (the snatch tuning's `yank`) and is armed only while the grip holds a remote target with no line out, so it never meets the zip's yank, which needs the button held on an attached line.

The grapple's aim sees weapons lying in the world, so the button works on a gun too ([grapple-snatch.md](grapple-snatch.md#the-grapple-button-at-a-gun)).

## The pull

The pull is measured along the grapple hand's aim at the tick it armed, in the cockpit frame (`grapple::physical::pull::LinePull`): it arms when the reel starts or the tether retract arms, takes the aim of the first tracked tick as its line, and reads nothing while disarmed. The aim suits both: a reel follows a fire or a tap along the line the hand pointed, and the retract follows a hold of the button at what the tethers hold. The grip's reach along the line is sampled every input tick, and the pull tracker reads two things from it:

- The pull's speed: how fast the hand comes back along the line, peak-held and decaying with `physical.pull.speed_hold_s` as its time constant, so that a pull at the start of a reel still counts through a reel longer than the arm's reach.
- The pull's depth: how far the hand is back from a reference that jumps out with the hand and creeps toward a hand held still at `physical.pull.relax_mps`. Reaching out and pulling back deepens it, holding the hand back lets it fade, and a hand-over-hand pull keeps it up.

An untracked hand forgets the pull, and a new arming starts it from the hand where it is.

## The reel

While the mode detector sees the character in the reel's wind-up or reeling in, the input tick publishes a drive: the scale from the pull's speed (`physical.reel.min_scale` with no pull, rising linearly to `physical.reel.max_scale` at `physical.reel.full_speed_mps`) and whether a boost runs. A detour on the reel's velocity update ([`NStateTask_ReelIn::UpdateReelVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-UpdateReelVelocity)) hands the local player's update a copy of the [`GrappleTuning`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrappleTuning) with the top speeds and accelerations scaled (`grapple::physical::reel`). The shared tuning resource is never written, and other characters' reels are untouched. The copy covers the bound reel fields, which are all the update reads of the tuning. A drive older than `physical.reel.drive_max_age_ms` is ignored, so the reel falls back to the game's own as soon as the input tick stops publishing. With `physical.reel.min_scale` at one, a reel the hand does not pull runs at the game's speed, and pulling only makes it faster.

A second detector with the zip's thresholds is armed only during the reel and measures along the hand's aim at the reel's start, so a boost is the zip's gesture again. A yank there runs the boost for `physical.reel.boost_s` of game time, so slow motion stretches it with the reel ([slow-motion.md](slow-motion.md)): the copy takes the tuning's retract top speed and acceleration in place of the plain ones, which is what holding the retract does for the game's boost. It applies only when [`GrapplingHook::GetRetractAcceleratesReelIn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetRetractAcceleratesReelIn) says the game's boost is unlocked. The boost does not touch the retract input, so it does not squeeze tethers. A long, fast pull that starts a zip can count as a boost too once the reel's wind-up arms the detector.

## The tether retract

The retract arms while the grapple button is held and the hook holds dual tethers ([`GrapplingHook::GetNumDualTethers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetNumDualTethers)), the grapple has no line of its own, nothing carries the character along a line, and the character is on the ground or falling. A line of the grapple's own is left out because the retract input pulls a light object on it toward the player, and a line being placed belongs to a tether not formed yet. The wingsuit and the parachute are left out because the retract input boosts them, and their arm steering and handles use the hands. On the tick it arms, the pull's depth starts from the hand where it is.

While armed, the pull's depth becomes the retract value (`grapple::physical::tether`): nothing inside `physical.tether.deadzone_m`, then the cube root of the pull's fraction of the span to `physical.tether.full_m`. The game squeezes by the cube of the retract value, so the squeeze follows the pull's depth in proportion; its own ramp over the hold time still applies. The value goes into the bindings' writes as an analog `RETRACT_GRAPPLE`, merged with the trigger's and the button's tightening, the larger winning.

## The hang swing

The hang task's detour pushes a hanging body against the grapple hand's displacement from where it took the line (the hold). The pump (`grapple::physical::swing`) adds a push against the hand's velocity relative to the head, smoothed over `physical.swing.smoothing_s`: zero below `physical.swing.deadzone_mps`, rising to `physical.swing.scale` times the game's own full-stick swing impulse ([`GrapplingHangTask_SmallImpulseSwingMagnitude`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHangTask_SmallImpulseSwingMagnitude)) at `physical.swing.full_mps`. Pulling the hand back as the body swings forward and reaching out as it swings back feeds the swing. The hold and the pump together are capped at `physical.swing.max_scale` times the game's impulse per hang tick, and each goes to the hang body at the hips.

## Haptics

The fire, the attach, and the yank play their cues on the grapple hand (`Event::GrappleFire`, `Event::GrappleAttach`, and `Event::Yank`), and a retract the pull starts plays `Event::TetherRetract` once. Every input tick also sends the grapple hand two levels, which the mixer sums: `Sustain::GrappleTension`, the tethers' highest tension ([`GrapplingHook::GetHighestWireTension`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetHighestWireTension)) while the game squeezes them ([`GrapplingHook::m_IsSqueezingDualTethers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##field-m_IsSqueezingDualTethers)), and `Sustain::GrappleReel`, the reel's speed over its top speed while the hand drives the reel. The game's own rumble for the squeeze and the reel still reaches the hands through the forwarded motors ([haptics.md](haptics.md)).

## Threading

The input tick's game stage (the game thread, before the world update; [bindings.md](bindings.md#injection-timing)) reads the hook, steps the pull, publishes the drive, and sends the pulses after releasing its own lock. The reel's detour runs in the character's update on the game thread and reads the drive from a leaf lock. The readout is published to its own leaf lock, so the `Grapple` status rows never wait on the input tick ([threading.md](../threading.md)).

## Readouts

The debug UI's Status page has a `Grapple` section, and the Grapple feature's readout opens with the same rows, then adds the attach point, the hang's hand motion, impulse, and pump, the hand's pull speed and depth, and the yanks:

- `Grapple state`: the hook's phase, or waiting while the input tick does not run.
- `Attached target`: an object or a surface, with the distance from the head to the attach point.
- `Reel from hand`: the scale while the hand drives a reel, whether a boost runs, and the counts of reels and boosts.
- `Tethers`: how many are attached, the retract value while the pull drives it, the tension while the game squeezes, and the count of pulls.
- `Line lock`: the lock ([grapple-lock.md](grapple-lock.md#readouts)).
- `Seat grapple`: the seat grapple ([seat-grapple.md](seat-grapple.md#readouts-and-logs)); on the Status page only.
- `Hang swing`: the hang ticks that swung, and how many the pump added to.
- `Snatch`: the snatch ([grapple-snatch.md](grapple-snatch.md#readouts)).
- `Line haptics`: the grapple hand's line level.

The log has, at info level under the `grapple` target, `physical grapple: the line attaches` with the target kind and distance, `the hand drives the reel` with the scale and the pull, `a yank boosts the reel`, `the reel ends` with the peak scale, `the hand retracts the tethers` with the count and value, `the tether retract ends` with the peak value, and `the tether count changes`.

## Headset checklist

1. Grapple to a wall and let the line hang without pulling: `Attached target` reads a surface with a plausible distance, `the line attaches` is logged, and the attach pulse is felt.
2. Hold the grapple button and yank to zip, then keep the hand still: `the hand drives the reel` with a scale near the yank's speed, and the reel is at least as fast as the gamepad's zip; `the reel ends` follows.
3. Zip again and pull hard and steadily through the reel: a higher peak scale in `the reel ends`, a faster zip, and a stronger hum in the grapple hand.
4. With the retract boost upgrade bought, yank again during a long reel: `a yank boosts the reel`, the row says boosted, and the reel visibly speeds up. Without the upgrade, nothing changes speed.
5. Tether two objects (point and hold the grapple button at the first, then at the second, and let go): `Tethers` reads one attached. Aim at nothing and hold the button: the tethers tighten, harder over a second. Still holding, reach out and pull back slowly: `the hand retracts the tethers`, the objects draw together faster, the retract pulse and then a hum that grows with the tension. Let go of the button: the tightening stops. The L trigger still retracts on its own. Double tap the button: the tethers release, and nothing fires.
6. With tethers attached and the button up, walk around and pull the hands back sharply: no retract and no zip.
7. Grapple onto a ledge into a hang and swing the arm forward and back in rhythm: the body swings and builds up, and `Hang swing` counts pumped ticks. Then move the hand wildly: the swing stays bounded.
8. During a long reel with the grapple button still held, spread both arms out to the sides as for the wingsuit: no `a yank boosts the reel`, and the reel keeps its speed. A sharp pull back toward the body still boosts.

## Unverified in the headset

- That the reel's velocity update reads nothing of the tuning beyond the bound reel fields in every path, including the vehicle entries and the hang approach.
- How the scaled reel feels at the defaults, and whether `physical.reel.min_scale` below one (a reel that slows when not pulled) is better.
- That the retract input's analog value drives the squeeze from the bindings' write as it does from the trigger.
- How hard the pump should push, and whether the hand's velocity relative to the head rather than to the line is the right measure while the body itself swings.
- Whether a hand pulled hard during the zip's wind-up gives an unwanted boost.
