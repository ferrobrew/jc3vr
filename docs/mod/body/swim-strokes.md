# Swimming with the arms

In water the player can swim by stroking with their arms, alongside the left stick: a hand sweeping back through the water, palm first, swims forward, and a stroke on one side alone turns the swimmer away from it. The code is `payload/src/controllers/locomotion/strokes/`, run from the bindings' finish stage while the swim gate reads swimming (`hooks::input::swim::swimming`, the motion state or the swim state bits; see [swim-yaw.md, "What swimming means"](swim-yaw.md#what-swimming-means)). It acts only under the controller scheme with tracked hands, so gamepad play and head-tracked gamepad play swim as before.

## What moves the swimmer

The game moves a swimmer only the way it faces ([swim-locomotion.md](../../engine/gameplay/swim-locomotion.md#orientation-application)): both swim cores turn the body toward a heading and swim along it at the speed the move stick asks for. The strokes therefore drive the same two inputs the sticks do, rather than pushing the body directly: a forward on the move stick, and a turn of the heading the right stick owns.

Each hand is a paddle. Its velocity in the cockpit frame, which rides the body, is its motion relative to the swimmer, smoothed over a few hundredths of a second against tracker jitter. A hand moving faster than `min_speed_mps` through the water pushes the swimmer the opposite way, as far as its palm faces the way it moves: full with the palm square to the motion, fading to nothing at `palm_facing_deg` from it, so a palm slicing edge-on through its recovery pushes nothing. The palm's normal is the wrist frame's palm side. A hand is in the water when it is more than `water_below_eyes_m` below the eyes at the surface, and always underwater (the `SB_IS_UW_SWIMMING` state bit). A hand holding a gun strokes nothing.

The push's share along the body's forward feeds a forward drive: each metre of palm-first paddle travel back through the water adds `gain` full sticks, and the drive decays with the time constant `decay_s` between strokes, the glide. It is capped at one and a half sticks, so a steady rhythm holds full speed through the recovery. A palm pushed forward against the water brakes the drive; it never goes below zero, since the swim has no backstroke. The drive, up to a full stick, is written to the move stick's forward, the larger of it and the stick's own forward winning, so the left stick still swims and the swim's backward and sideways refusal still applies.

The push also turns the swimmer about the head: each hand's offset from the head crossed with its push, summed. That turn is smoothed with the time constant `turn_smoothing_s`, so the alternate arms of a crawl cancel and a breaststroke's mirrored sweeps cancel outright, while a stroke on one side alone turns the swimmer away from that side. The smoothed turn times `turn_gain`, limited to `max_turn_deg_s`, is added each tick to the heading the right stick owns (`headpose::xr::add_swim_turn`), which the swim's heading rotation turns the body toward ([swim-yaw.md](swim-yaw.md)). It is a turn the player makes with a movement, as the right stick's is. Underwater the dive still follows the head's pitch; the strokes do not pitch the swimmer.

A pause in the ticks longer than a quarter of a second, or leaving the water, forgets the hands' motion, so the first tick back does not read the gap as one fast stroke.

## Haptics

A stroke catches the water when its backward push passes `catch_mps`, and lets go once it falls below half that. Each catch knocks its hand (`Event::SwimStroke`, a bump scaled by how hard the stroke pulls; [haptics.md](../input/haptics.md)).

## Settings and readouts

The switch and the tuning are `controllers.locomotion.strokes` in the debug UI's On foot feature: `enabled` (on), `gain`, `decay_s`, `min_speed_mps`, `palm_facing_deg`, `catch_mps`, `turn_gain`, `turn_smoothing_s`, `max_turn_deg_s`, and `water_below_eyes_m`. The feature's live status shows, while swimming, each hand in or out of the water with its push, the strokes caught per hand, the drive, the forward written against the stick's, the smoothed turning push, and the turn this tick.

## Unverified

- The defaults: how far a stroke carries the swimmer against the game's swim speeds, and whether the glide and the turn feel right for a breaststroke and a crawl.
- That the game's surface and underwater cores swim at a speed that follows the move stick's magnitude, rather than a fixed swim speed past a threshold.
- The water line at the surface, taken as a fixed distance below the eyes rather than the water's height.
- That a turn of the crawl's alternate arms is smoothed out enough not to sway the view.

## Headset checklist

1. At the surface with the stick released, do a breaststroke with both hands: the swimmer moves forward, glides, and slows within a couple of seconds; each stroke knocks both hands; the heading holds.
2. Crawl, one arm after the other: the swimmer moves forward without visibly swaying side to side.
3. Stroke with the left hand alone: the swimmer turns right. With the right hand alone: left.
4. Hold the hands still, or move them slowly: nothing moves. Bring the hands forward for the next stroke with the palms down: the swimmer is not slowed. Bring them forward palms first: it is.
5. Lift the hands above the water and stroke: nothing at the surface. Dive, and stroke: the swimmer moves forward underwater, and looking up while stroking climbs.
6. Push the left stick forward while stroking: the larger wins; release it: the strokes still swim. Hold a pistol in one hand: that hand strokes nothing, the other still does.
7. Walk out of the water: no stroke carries over onto foot.
