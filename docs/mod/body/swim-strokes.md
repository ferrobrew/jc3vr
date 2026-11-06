# Swimming with the arms

In water the player can swim by stroking with their arms, alongside the left stick: a hand sweeping back through the water, palm first, swims forward, and a stroke on one side alone turns the swimmer away from it. The code is `payload/src/controllers/locomotion/strokes/`, run from the bindings' finish stage while the swim gate reads swimming (`hooks::input::swim::swimming`, the motion state or the swim state bits; see [swim-yaw.md, "What swimming means"](swim-yaw.md#what-swimming-means)). It acts only under the controller scheme with tracked hands, so gamepad play and head-tracked gamepad play swim as before.

## What moves the swimmer

The game moves a swimmer only the way it faces, at the speed of the animation's root motion ([swim-locomotion.md, "Swim speed"](../../engine/gameplay/swim-locomotion.md#swim-speed)): the stick's magnitude only picks between the crawl and its start act, so a stick written at half swims no slower than a full one, and below the start threshold it never starts the crawl from treading water. The strokes therefore do not drive the stick. They keep up a swim speed of their own, which the swim cores' detours join to the velocity each core wrote, along the way the swimmer faces, and they turn the heading the right stick owns.

Each hand is a paddle. Its position is the player's own hand, where the controller holds it in the cockpit frame (`hands::controller_grips`), not the drawn hand: the arm remap and the reach limit stop the drawn hand short near full reach, which is where a stroke catches, and would take about half of each stroke's push. Its velocity in the cockpit frame, which rides the body, is its motion relative to the swimmer, smoothed over a few hundredths of a second against tracker jitter. A hand moving faster than `min_speed_mps` through the water pushes the swimmer the opposite way, as far as its palm faces the way it moves: full with the palm square to the motion, fading to nothing at `palm_cone_deg` from it, so a palm slicing edge-on through its recovery pushes nothing. The cone is wide because a hand closed round a controller rarely holds its palm square to the stroke. The palm's normal is the wrist frame's palm side. A hand is in the water when it is more than `water_below_eyes_m` below the eyes at the surface, and always underwater (the `SB_IS_UW_SWIMMING` state bit). A hand holding a gun strokes nothing.

The push's share along the body's forward feeds the swim speed: each metre of palm-first paddle travel back through the water adds `speed_per_metre` metres per second, and the speed decays with the time constant `decay_s` between strokes, the glide, up to `max_speed_mps`. A stroke is a burst that arrives while the hand pulls; a steady rhythm holds a speed that rises with how hard and how often the hands pull. A palm pushed forward against the water brakes at half strength, since a recovery swept forward with the palm half turned reads as a push forward; the speed never goes below zero, since the swim has no backstroke.

After each swim core runs for the local player, its detour reads the velocity the core wrote to the proxy back into the world through the proxy's transform, raises its share along the way the swimmer faces to the strokes' speed, and writes it back with `Character::SetWantedWorldVelocity` (`hooks::input::swim::propel_strokes`, the pure part `strokes::model::propel`). The facing is the heading the core's own rotation produced that run: level at the surface, so the core's buoyancy keeps the vertical, and pitched underwater, so the strokes swim where the dive points. Everything else the core wrote stays: the drift across, the buoyancy, the waves' push. A core already faster along the facing, the left stick's crawl, keeps its own speed, so the stick still swims. The core still picks its own animation and capsule pose from the stick, so a swimmer moved by the strokes alone treads water upright while it glides; the arms follow the player's hands either way ([procedural-states.md](procedural-states.md#swimming)).

The push also turns the swimmer about the head: each hand's offset from the head crossed with its push, summed. That turn is smoothed with the time constant `turn_smoothing_s`, so the alternate arms of a crawl cancel and a breaststroke's mirrored sweeps cancel outright, while a stroke on one side alone turns the swimmer away from that side. The smoothed turn times `turn_gain`, limited to `max_turn_deg_s`, is added each tick to the heading the right stick owns (`headpose::xr::add_swim_turn`), which the swim's heading rotation turns the body toward ([swim-yaw.md](swim-yaw.md)). It is a turn the player makes with a movement, as the right stick's is. Underwater the dive still follows the head's pitch; the strokes do not pitch the swimmer.

A pause in the ticks longer than a quarter of a second, or leaving the water, forgets the hands' motion and the speed, so the first tick back does not read the gap as one fast stroke and no glide carries onto foot.

### Calibration

The defaults come from the strokes in the 6 November recording, replayed through the model on the raw hand tracks. The hands there peaked at 2 to 4 m/s relative to the head. A 50-degree palm cone with full-strength braking gives a vigorous breaststroke about 0.4 m/s of forward paddle travel on average and a later stretch of mixed strokes about 0.08; the defaults' wide cone and half braking give about 0.54 and 0.22. With `speed_per_metre` 4.5 and the 1.5 s glide those settle near 3.7 and 1.5 m/s, against the crawl's top speed of about 4 m/s.

## Haptics

A stroke catches the water when its backward push passes `catch_mps`, and lets go once it falls below half that. Each catch knocks its hand (`Event::SwimStroke`, a bump scaled by how hard the stroke pulls; [haptics.md](../input/haptics.md)).

## Settings and readouts

The switch and the tuning are `controllers.locomotion.strokes` in the debug UI's On foot feature: `enabled` (on), `speed_per_metre`, `decay_s`, `max_speed_mps`, `min_speed_mps`, `palm_cone_deg`, `catch_mps`, `turn_gain`, `turn_smoothing_s`, `max_turn_deg_s`, and `water_below_eyes_m`. The feature's live status shows, while swimming, each hand in or out of the water with its push, the strokes caught per hand, the strokes' speed, the speed along the facing the swim core wrote against the speed written in its place, the smoothed turning push, and the turn this tick.

## Unverified

- The defaults against a real swim: how far a stroke carries, and whether the glide and the turn feel right for a breaststroke and a crawl.
- That the proxy honours the raised velocity at the surface as it does the core's own, and that the core's buoyancy is unchanged by it.
- The water line at the surface, taken as a fixed distance below the eyes rather than the water's height.
- That a turn of the crawl's alternate arms is smoothed out enough not to sway the view.
- How a stroke taken while the grapple reels the swimmer in, or while a planted explosive's warp runs, combines with the warp's velocity: the strokes raise the warp's share along the facing like any other.

## Headset checklist

1. At the surface with the stick released, do one breaststroke with both hands: the swimmer surges forward during the pull, glides, and slows to a stop within a few seconds; each stroke knocks both hands; the heading holds.
2. Keep stroking at an easy rhythm, then harder: the swimmer settles at an easy pace, then a clearly faster one; the debug UI's stroke speed rises with the effort and the written speed follows it.
3. Crawl, one arm after the other: the swimmer moves forward without visibly swaying side to side.
4. Stroke with the left hand alone: the swimmer turns right and still moves forward. With the right hand alone: left.
5. Hold the hands still, or move them slowly: nothing moves. Bring the hands forward for the next stroke with the palms down: the swimmer is not slowed. Bring them forward palms first: it slows, but less than a stroke speeds it up.
6. Lift the hands above the water and stroke: nothing at the surface. Dive, and stroke: the swimmer moves forward underwater, and looking up while stroking climbs.
7. Push the left stick forward while stroking: the faster wins; release it: the strokes still swim. Hold a pistol in one hand: that hand strokes nothing, the other still does.
8. Walk out of the water: no stroke or glide carries over onto foot.
