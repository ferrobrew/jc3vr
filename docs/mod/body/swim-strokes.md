# Swimming with the arms

In water the player can swim by stroking with their arms, alongside the left stick: a hand sweeping back through the water, palm first, swims forward, a breaststroke's outward sweep adds to it, and a stroke on one side alone turns the swimmer away from it. The code is `crates/jc3vr-body/src/controllers/locomotion/strokes/` (the pure part in `strokes::model`), run from the bindings' finish stage while the swim gate reads swimming (`hooks::input::swim::swimming`; [swim-yaw.md, "What swimming means"](swim-yaw.md#what-swimming-means)). It acts only under the controller scheme with tracked hands, so gamepad play and head-tracked gamepad play swim with the stick alone. The strokes have no settings; their tuning is the `strokes` section of `crates/jc3vr-body/tuning/locomotion.toml` ([tuning.md](../tuning.md)), whose keys this page names.

## What moves the swimmer

The game moves a swimmer only the way it faces, at the speed of the animation's root motion ([swim-locomotion.md, "Swim speed"](../../engine/gameplay/swim-locomotion.md#swim-speed)): the stick's magnitude only picks between the crawl and its start act, so a stick written at half swims no slower than a full one, and below the start threshold it never starts the crawl from treading water. The strokes therefore do not drive the stick. They keep up a swim speed of their own, which the swim cores' detours join to the velocity each core wrote along the way the swimmer faces, and they turn the heading the right stick owns.

## The paddles

Each hand is a paddle. Its position is the player's own hand, where the controller holds it in the cockpit frame (`hands::controller_grips`), not the drawn hand: the arm remap and the reach limit stop the drawn hand short near full reach, which is where a stroke catches. Its velocity relative to the head in the cockpit frame, which rides the body, is its motion through the water, smoothed over `velocity_smoothing_s` against tracker jitter. Strokes are read against the estimated torso's forward laid flat, not the cockpit's. A hand is in the water when it is more than `water_below_eyes_m` below the eyes at the surface, and always underwater (the `SB_IS_UW_SWIMMING` state bit). A hand holding a gun strokes nothing.

Each hand goes through a stroke's phases. A hand moving forward faster than `min_speed_mps`, or one more than `finish_behind_head_m` behind the head, is recovering and pushes nothing, so the forward recovery never brakes the glide. A recovering hand that reaches `catch_reach_m` ahead of the head is ready. A ready hand that paddles `catch_travel_m` catches the water, and pushes from then on until it recovers.

A paddling hand's drive is its speed backward along the forward, plus, while both hands sweep outward ahead of the head (a breaststroke), the share `breaststroke_sweep_share` of its outward speed, less `min_speed_mps`. The palm scales it: full with the palm square to the motion, falling to the assistance `min_alignment` at `palm_cone_deg` from it, and nothing with a palm facing against the motion (below the cosine `palm_against_cos`). The cone is wide and the assistance is never zero because a hand closed round a controller rarely holds its palm square to the stroke. The palm's normal is the wrist frame's palm side.

The hands' summed drive feeds the swim speed: each metre of paddle travel adds `speed_per_metre` metres per second, and the speed decays with the time constant `decay_s` between strokes (the glide), up to `max_speed_mps`. A steady rhythm holds a speed that rises with how hard and how often the hands pull.

Leaving the water, losing the tracked hands, or a pause in the ticks longer than the freshness tuning's `locomotion.strokes_ms` resets the strokes, speed included, so no glide carries onto foot and the first tick back does not read the gap as one fast stroke. A single tick longer than `locomotion.strokes_max_tick_s` forgets the hands' motion and their phases the same way.

## Joining the swim core's velocity

After each swim core runs for the local player, its detour reads the velocity the core wrote to the proxy back into the world through the proxy's transform, raises its share along the way the swimmer faces to the strokes' speed, and writes it back with `Character::SetWantedWorldVelocity` (`hooks::input::swim::propel_strokes`, the pure part `strokes::model::propel`). The facing is the heading the core's own rotation produced that run: level at the surface, so the core's buoyancy keeps the vertical, and pitched underwater, so the strokes swim where the dive points. Everything else the core wrote stays: the drift across, the buoyancy, the waves' push. A core already faster along the facing, the left stick's crawl, keeps its own speed, so the stick still swims. The core still picks its own animation and capsule pose from the stick, so a swimmer moved by the strokes alone treads water upright while it glides; the arms follow the player's hands either way ([procedural-states.md](procedural-states.md#swimming)).

## Turning

The strokes turn the heading two ways, summed and limited to `max_turn_deg_s`:

- The push turns the swimmer about the head: each hand's offset from the head crossed with its push, summed and smoothed with the time constant `turn_smoothing_s`, times `turn_gain`. The alternate arms of a crawl cancel in the smoothing and a breaststroke's mirrored sweeps cancel outright, while a stroke on one side alone turns the swimmer away from that side.
- The heading follows the way the hands push, since the game swims a swimmer only the way it faces and a stroke toward one side would otherwise move it straight ahead. Each in-water hand's level push (the opposite of its motion, weighted by its palm's alignment) counts while it points ahead of the body's side, a sweep straight to the side included. The two hands' pushes are summed, so a breaststroke's mirrored sweeps cancel, and the sum's angle off the forward turns the heading toward it at a rate that carries it through that angle over `steer_travel_m` of push, about one stroke's pull. The rate is smoothed with `turn_smoothing_s`, so a crawl's alternate sweeps cancel too. A stroke pulling back and to the left turns the swimmer right by about the stroke's angle; the speed it adds is still its share along the facing.

The strokes queue the turn, and the head pose takes it on its next input tick (`strokes::take_turn`, one of the head pose's participants) and adds it to the heading the right stick owns, which the swim's heading rotation turns the body toward ([swim-yaw.md](swim-yaw.md)). It is a turn the player makes with a movement, as the right stick's is. Underwater the dive still follows the head's pitch; the strokes do not pitch the swimmer.

## Haptics

Each catch knocks its hand (`Event::SwimStroke`; [haptics.md](../input/haptics.md)), with a strength of the paddle's speed against `catch_mps`, never weaker than `catch_min_strength`.

## Readouts

The debug UI's On foot feature's live status shows, while swimming, each hand in or out of the water with its push, the strokes caught per hand, the strokes' speed, the speed along the facing the swim core wrote against the speed written in its place, the smoothed turning push, and the turn this tick. The swim telemetry line carries the same values ([swim-yaw.md, "Telemetry"](swim-yaw.md#telemetry)).

## Unverified

- The defaults against a real swim: how far a stroke carries, and whether the glide and the turn feel right for a breaststroke and a crawl.
- That the proxy honours the raised velocity at the surface as it does the core's own, and that the core's buoyancy is unchanged by it.
- The water line at the surface, taken as a fixed distance below the eyes rather than the water's height.
- That a turn of the crawl's alternate arms is smoothed out enough not to sway the view.
- How a stroke taken while the grapple reels the swimmer in, or while a planted explosive's warp runs, combines with the warp's velocity: the strokes raise the warp's share along the facing like any other.

## Headset checklist

1. At the surface with the stick released, one breaststroke with both hands: the swimmer surges forward during the pull, glides, and slows to a stop within a few seconds; each stroke knocks both hands; the heading holds.
2. Stroking at an easy rhythm, then harder: the swimmer settles at an easy pace, then a clearly faster one; the debug UI's stroke speed rises with the effort and the written speed follows it.
3. A crawl, one arm after the other: the swimmer moves forward without visibly swaying side to side.
4. Stroking with the left hand alone turns the swimmer right while it still moves forward; the right hand alone turns it left.
5. Holding the hands still, or moving them slowly, moves nothing. Bringing the hands forward for the next stroke, palms down or palms first, does not slow the swimmer.
6. Stroking with the hands above the water does nothing at the surface. Diving and stroking moves the swimmer forward underwater, and looking up while stroking climbs.
7. Pushing the left stick forward while stroking: the faster wins; releasing it, the strokes still swim. Holding a pistol in one hand: that hand strokes nothing, the other still does.
8. Walking out of the water: no stroke or glide carries over onto foot.
9. Stroking diagonally, both hands pulling back and to one side: the swimmer turns toward the side the stroke pushes, by about the stroke's angle, then swims that way. A breaststroke and a crawl straight ahead still hold the heading.
