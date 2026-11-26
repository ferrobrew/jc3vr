# Body and legs: the 14 November feedback

Written on 15 November from the take and the code as they were before the fixes listed in the [README](../README.md#the-feedback-item-by-item); file and line references are to that code. Scripts, dumps, and frame sheets it names were working files of the analysis and are not kept.

Times are t (the Rerun `recording` timeline); video time is t − 1.864 s. The data comes from the arrays the initial harvest extracted from `tracking.jc3trk` (`session1114/body/t14.npz` and `t14_other.jsonl`: the PreIk and final poses with all 23 bones, the controller states, the rig records, the body states), the harvest's `rig-eval --pelvis recorded --weights` output (`rigeval.csv`) and `estimate-body` output (`est.csv`, the EgoPoser estimate replayed over the whole take), the session logs of this take and of 10 November, and video frames. Scripts and frames are in `analysis/body/` in the scratchpad (`chest*.py`, `lean.py`, `belly.py`, `win.py`, `rge.py`, `offs.py`, `legs.py`, `footik.py`, `landings.py`, `climb.py`, `yawmove.py`, `yawest.py`, `hipswing.py`, `estlegs2.py`, `perf.py`, and the `grid_*.jpg` frame strips).

One convention matters throughout: Rico's model space faces −Z with the hips at the horizontal origin, so "forward of the camera" below is measured along Rico's facing, with the camera expressed in the model space of the same simulation pass (the camera's `current` pose against the pose of that pass, which removes the interpolation lag that otherwise shows up as ±10 cm at sprint speed).

## 278 s and 280 s: the chest comes forward while walking

What happened. At 276–282 the player looks down 40–50° while running backward (276–279.5, up to 5.6 m/s) and then forward (279.7–282). The video crop does not reach Rico's chest (the bottom of those frames shows the held launcher), so this rests on the poses. The clip and the solved pose, by gait, over the whole take:

| | upper spine lean, clip (PreIk) | upper spine lean, final | hips→Spine1 lean, clip | final |
|---|---|---|---|---|
| standing | −2° | −2 to +5° | about 0 | about 0 |
| walking forward or back | +18 to +24° | −3 to +5° | +20 to +22° | +10 to +12° |
| sprinting, head level | +26° | −8 to −10° | +22° | +11° |
| 276.5–282 (the feedback) | +24 to +28° | −13 to −23° | +21 to +23° | +10 to +13° |

(Positive is leaning forward. The upper spine is Spine1 to Neck; the lower segment is the Hips joint to Spine1.)

Every locomotion clip, backward ones included, leans Rico's whole torso 20–27° forward. The procedural torso takes the neck back under the player's head, and it does: the clip's head bone is 3–12 cm ahead of the camera in a walk or sprint, the final one 10–11 cm behind it, as standing. But the swing never moves the pelvis. The lower spine keeps half the clip's forward tilt (11°), so to bring the neck back the upper spine is bent backward, by 8–10° at a level head and 13–23° in the feedback's stretch. The torso ends up kinked: belly pushed forward over the hips, chest face tipped up toward the eyes. Spine1 and Spine2 sit 3–4 cm further forward against the camera than standing (−10 cm against −13.4 cm), and the chest's front surface, which faces horizontally when Rico stands, faces 10–20° upward. A look down at a chest that is tipped back toward you shows much more of it. That fits "pushes my chest in front, taking up more of my field of view", and the reviewer's hypothesis is right in substance: it is the clip's forward lean, which the solver hides from the neck by bending the chest back on top of the clip's tilted pelvis. The sternum joint itself is not further forward than standing (−11 to −15 cm), so measured by joint positions alone it looks like nothing changed.

The near-field guard does not catch it. `keep_chest_clear` (`payload/src/controllers/body/procedural/solver/anchored.rs:136`) only acts on chest points within `chest_clear_below_m` (0.25 m, `procedural/config.rs:69`) below the eyes, and Rico's whole body drops about 5 cm in a gait (hips 1.01 → 0.96 m), which puts the sternum 28–30 cm below the camera, outside the band.

Root cause (code): `solve_anchored` keeps the clip's pelvis and swings only `SPINE` = Spine, Spine1, Spine2, Sternum about the Spine joint with `SWING_SHARES` (`solver/anchored.rs:55` and `:87–99`, `solver/spine.rs:63–71`). The swing's `upright` is the clip's own spine line, so the clip's forward lean is the reference the swing departs from, and the Spine bone's 20% share is all the lower torso gets back.

Fix. In `solve_anchored`, before the twist, de-lean the clip on foot: rotate the spine chain about its base so the clip's base-to-neck line goes to vertical by a share (say 80%, `procedural.on_foot_delean`), shared toward the lower bones (Spine and Spine1 taking most of it), and only then twist and swing toward the neck target. The swing then adds only the player's own lean and the chest stays square over the pelvis. Gate it on the on-foot case (seats and the harness should keep their clips' posture). Alternatively, and better in the IK-first design below, the ~20% IK blend onto the walk includes the pelvis's pitch, so part of the clip's pelvis tilt is taken out at the hips rather than kinked out of the chest. Also widen the near-field guard's band to follow the body's drop in a gait (measure `below` from Rico's standing chest height rather than from the eyes). Nothing here needs reverse engineering. This also bears on the harvest's finding that the head bone runs 10–12 cm off the neck pivot while running: the swing's lean limit is measured from the clip's leaning spine, and a de-leaned base would bring the head onto the pivot in a gait too.

## 132–140 s: sliding, misalignment, and a slow catch-up

What happened. From 130.5 to 139.5 the player stands looking down at their feet, 55–77° of pitch, and bends a little (the neck pivot drops 6–29 cm). The rig reads the neck moving out over the pelvis as a lean, and the lean's reach grows with the neck's drop (0.31–0.53 m here), so the pelvis stays put while the neck is 0.18–0.32 m out from it for 3–4 s (131.0–135.0) with the spine bent 16–30°. A step commits at 135.0 and the pelvis closes 0.27 m in under half a second, then again at 136.0 and 138.0, and the settle at 138.5–139.0 slides the remainder with no walk. The frames show Rico's feet in view at 132–135, then at 135.5–136 the camera inside the shoulders and harness right after the step, and a grey near-clip frame at 137.5.

This is general, not this moment's: while idle and looking down past 55°, the neck sits a median 14 cm from the pelvis (p90 30 cm), more than 15 cm for 67 of 154 s and more than 25 cm for 20 s. Looking down is exactly when the player sees the body, so the misalignment is concentrated where it is visible. Across the take, catch-up mostly happens by sliding: the idle drift ran 98 s and moved the pelvis 12 m with no walk, against 17 s of committed steps for 8 m.

Root cause. Three things together. The lean's cap is `pelvis.max_lean_m` = 0.6 (`crates/body-rig/src/pelvis.rs:68`, used at `:197`), so a low neck may be 0.6 m out without a step. The neck pivot is a fixed offset from the eyes in the head's frame, and a real 70° look down also bends the neck and upper back, so the pivot moves forward and down, which the solver reads as a bend at the waist. And the catch-up is a hysteresis step (5 cm commit past the lean) followed by slides (`step.drift_speed_mps` 0.12, `step.settle_speed_mps` 0.15), which read as the body skating under the player.

Fix. In the IK-first design the question goes away: the pelvis goes where the estimate's forward kinematics puts it (behind the head when bending over to look down), the feet stay planted, and the root drive targets the floor point under that pelvis, so a bend is not a gap and there is no catch-up event; what the player sees catching up is a foot stepping. Until then, in the current rig: lower `max_lean_m` to about 0.35, have the look-down move the neck pivot back by the head-pitch-dependent forward shift a real neck makes (the rig already knows the head's pitch), and replace the drift's and the settle's slides with a short step (the walk stick for a few frames, as `grounded::add_step_walk` does for steps, `rig/grounded.rs:198`) so the catch-up is a visible step rather than a slide. That last part is what "make the catch-up more evident" asks for.

## 899.7–900.2 s: the legs do not fit the ground

What happened. At 897.5 Rico lands from a fast fall on a steep snowy slope and plays `S_LAND_FAST_FALL` (a kneeling landing and get-up) until 901.65. The player stands throughout. In the frames at 899.7–900.2 a leg is folded up with the knee near the camera. From 899.5 to 901.1, during the get-up, the final pose's feet move 30–45 cm horizontally away from the clip's feet while the clip's feet stand still (left foot: clip −0.29, −0.10; final swings out to −0.55, +0.11 and round to −0.22, +0.34), and their height never changes (the PreIk and final foot heights agree to a millimetre). So something drags the landing clip's feet across a slope at their old height: one floats over the downhill side and the other sinks into the uphill side.

Root cause. The square stance. `stance::place_feet` (`payload/src/controllers/body/procedural/stance.rs`, the function after `StanceBlend`) moves the game's ankle and toe targets across the ground "keeping each target's height", gated in `procedural/mod.rs:338–356` on `case == Case::OnFoot` and `locomotion.standing_still()`. The fast-fall landing is not classified as an authored motion: the log has the case go from "a fall past the jump's grace" straight to "on foot" at 897.648, and `AUTHORED_MOTION` (`procedural/state.rs:191`) lists `SB_IS_LANDING_HARD`, which this state evidently does not set. Once the get-up's root stops moving, `standing_still` is true and the stance blends in at a 0.25 s time constant toward a square stance under hips that the clip is still rotating, so the feet slide. The same happens at 6 of the 18 fast-fall landings (870.1, 897.6, 908.9, 1410.1, 1418.9, 1807.2), one of the two `S_LAND_HARD`, and 38 of 120 `S_LAND_IDLE_0` (by 15–22 cm). The keep-the-height rule is the second half of the bug: on a slope the height belongs to the old footprint.

Fix. Gate the stance on the base idle state (the body state's `S_IDLE`, which the recorder already names), not only on the case and the speed; add the fast-fall landing to the authored set (find which state bit `S_LAND_FAST_FALL` sets, or match the state by name as `hooks/camera/authored/native.rs:614` already does); and re-probe the ground height at the stance's new x/z instead of keeping the old target's height. In the IK-first design the stance disappears into the foot-contact model, which probes every planted foot where it lands. The reverse engineering needed is only which state bits the fast-fall landing sets.

## 1313 s and the next 30 s: climbing a cliff

What happened. From 1309.6 to past 1350 the player hand-climbs an inclined rock face (root owner `climb`; Rico rises 10 m in 40 s). Throughout, the camera stands 0.5–0.6 m horizontally ahead of Rico's root, against 0.12 m on foot, with the arms reaching a further 0.6 m ahead. The rig does not step while a hand grips and nothing else moves the root toward the head, so the offset is never absorbed. Rico's legs are the game's idle clip standing in the air (the climb writes the proxy as supported and level, `docs/mod/input/hand-climbing.md`, "Keeping the game on foot"), so the feet hang off the face; the frame at 1313 shows a dangling boot below the view, and from 1325 the camera skims the slope's surface.

Why the player has to keep pushing forward: on an inclined face, the face at head height is further away horizontally than at the feet. The climb moves the body opposite to the hand (pull down, rise), which is purely vertical for a vertical pull, so each pull lifts Rico to where the face has receded, and the capsule's feet against the slope keep it out. A person scrambling up such a slope folds at the hips with the feet on the rock; the player, standing upright, has to bring the whole head forward to reach and see. The room position of the head stays about 0.9 m from the cockpit origin for the whole climb, so this is a sustained forward stance rather than a creeping walk.

Proposals, in order of preference:

1. Make the climb follow the surface. When a gripped surface's normal is inclined, add to the climb's target the horizontal motion toward the face that keeps the body at a constant standoff from the grip along the normal (for a pull that rises dy on a face whose normal has horizontal and vertical parts n_h and n_y, about dy · n_y / |n_h| toward the face). The body then rises along the slope and the head-to-face distance stays what the player set at the grip. This is in `payload/src/controllers/physical_hands/climb/` and needs no reverse engineering (the grip already records the normal).
2. Gentle recentering while gripping: when the head-to-root horizontal offset stays above about 0.3 m for a second during a climb, slide the cockpit shift so the root closes on the head at a few centimetres a second, applied only while the hands are pulling (so the change rides on the player's own motion and never moves the view at rest). The push-back's climb threshold (0.45 m) stays as the safety. This is a translation of the room, not a reorientation, so it respects the rule against reorienting the player without a movement action.
3. Feet on the face. Cast from each hip toward Rico's facing and from each ankle forward and down; where a hit lies within the leg's reach, put the ankle and toe HumanIK targets on it with the sole against the normal and the knee hinted toward the wall, and otherwise let the leg hang relaxed. This is the same contact machinery as the IK-first idle below (brace only on valid surfaces, otherwise a hanging pose). The rig already casts rays (`rig/collision`), so this needs no new reverse engineering.

## 2044–2050 s: body yaw while moving, and the belt

What happened. Three reload pouch grabs missed while sprinting at 5.4 m/s, at 2043.65 (103 mm out of reach), 2044.48 (51 mm), and 2045.82 (22 mm); the fourth, standing still at 2050.81, took. During the misses Rico faced 42° (rig yaw), the head turned from 37° to 33° and the player looked down 50–74°, and the movement direction ran 14–3° to the head's side of Rico. So Rico's facing was 5–20° from the head and 3–15° from the movement: not large. Two other things were moving the pouch: the sprint clip swings Rico's pelvis −10 to +8° about his facing every stride and bobs the hips 10 cm (0.92–1.03 m), and the belt's volumes ride the hip joints' line (`controllers/weapons/volumes.rs`, `BodyFrame::pelvis`), so they swing and bob with the stride. Standing at 2047.5–2050.5 Rico turned to 22–23° right of the head, following the estimate's pelvis (68° against the head's 52°); the grab worked there.

The yaw offsets across the take (Rico's facing against the headset's heading and the movement direction):

| | time | Rico − head, median | p90 | over 30° | Rico − movement, median |
|---|---|---|---|---|---|
| standing | 1580 s | 15° | 41° | 21% | |
| walking | 289 s | 13° | 42° | 19% | 54° |
| jogging | 226 s | 12° | 39° | 17% | 38° |
| sprinting | 1093 s | 12° | 38° | 15% | 23° |
| moving, looking down past 35° | | 17° | 50° | 29% | |

Against the estimate's pelvis (the best guess at where the player's real belt is): standing, Rico is a median 8° off (p90 20°, 2% over 30°); moving, 12° (p90 32°, 12% over 30°). The difference is that the estimate's say is blended out while the stick walks (`yaw.estimate.stick_blend_s`, `crates/body-rig/src/estimate.rs:208`), so the head rules alone face Rico while he moves, and past `look_down_deg` (55°, `crates/body-rig/src/yaw.rs:98`) the turn rules stop except the large turn, which freezes Rico's facing while the player looks down at the belt and turns.

On the reviewer's proposal. Aligning the body with the movement direction would make things worse for the case that matters most here: the movement direction is the stick's, relative to the head, and the player strafes and backpedals a lot (the median movement direction is 23–54° off Rico's facing; 278 s is a backward run). Facing the movement would put the belt 90–180° from where the player's real belt is. What the data supports instead:

- While moving, hold Rico's facing to the head's heading more tightly (narrow `yaw.moving_deadzone_deg` from 25° and shorten `moving_sustain_s`), and keep the estimate's pelvis in charge of the belt's yaw while the stick walks, since the player's real body is standing still and the estimate of it stays valid (drop the stick blend-out for the belt, keep it for the legs).
- Take the stride out of the belt: give the belt and hip sockets a frame that follows the pelvis's yaw and height low-passed over a stride (or the IK pelvis below), rather than the clip's hip joints frame by frame. This is in `controllers/weapons/volumes.rs`, `BodyFrame::on`.
- Do not freeze the facing on a look down while moving; the look-down freeze exists for standing, where looking at a holster swings the head without the body turning.

These change the rig and the volumes only; no reverse engineering. Note that the pouch hint misses of 22 and 51 mm are within what the stride's bob alone explains.

## 1806 s: the camera inside the body

What happened. From 1805.1 to 1807.1 Rico fast-falls (after a canopy at 1803.3 and a fall from 1803.8) while the player stands. The game owns Rico's yaw in a fall, and it held −50° while the head looked at +20 to +43°: 70–93° apart. At 1805.37 the procedural body handed the torso to the animation (the log's case change with `torso=0.0`), so the fast-fall clip's pose was drawn, and its torso leans toward Rico's right, which with the yaw offset is toward where the camera looks. The camera sat 4 cm from the model's origin at 1.55 m with the head bone 6–25 cm and the sternum 4–8 cm from it; the frames at 1805.5–1807.0 are filled with the harness, the grenade pouch, and the shoulders. The reviewer's suspicion is right: it is the yaw offset, with the clip's lean.

Root cause. The flight hold, `FLIGHT_HOLD_S` = 1.5 s (`procedural/authority.rs:352`), after which a fall hands the torso to the animation; and the game's in-air facing, which the rig does not turn. At the landing (1807.26–1807.5) the yaw then snaps 37° and 40° in two quarter-second steps onto the head.

Fix. The binding answer covers it: airborne is IK'd. With the IK pose in the air (the torso and the legs following the player, Rico's yaw following the head, the pelvis under the head), the camera cannot be inside the torso. Concretely: do not hand the torso over after the flight hold in a plain fall (keep `Case::Flight` procedural), and turn Rico's drawn facing toward the head's heading while falling (the game's own facing still steers the air control, so the drawn body can be turned on the gate's pose rather than through the character's yaw if changing the yaw upsets the air steering, which is unverified). This also removes the landing's yaw snap.

## The estimator's findings in the harvest

The README's numbers (legs follow every glance with a median 23°, 55 leg turns a standing minute, motion in 57% of standing frames, turn-in-place shuffles at 16.4 a minute with 83% following a rig turn, the estimate swinging 64–84% of a head-only glance) are right, and the causes are in `crates/body-rig/src/estimate.rs`: the legs follow the smoothed estimate outside a 9° band (`yaw.estimate.deadband_deg`), and any frame they turn faster than `TURNING_DEG_S` = 10°/s (`estimate.rs:116`, applied in `yaw.rs:342`) is a "turn", which is the gate the act filter uses to let the game's turn-in-place acts through. So the estimate's glance-driven drift starts the game's animated foot pivots.

One more finding from replaying the estimate with forward kinematics on SMPL's mean skeleton (`estlegs2.py`): EgoPoser's own legs are not usable as foot placements. While the player stands still, its ankle line turns at a median 26°/s (p90 131°/s), follows every pelvis turn almost exactly (a ratio of 0.97 over half-second turns of more than 10°), and the ankles' separation wanders between 15 and 33 cm. Its pelvis is steadier (median 15°/s) and its knees do flex with a crouch (median 163°, 5th percentile 147°). So the estimate is good for the pelvis's heading, its height, and the knees' bend, and it carries the head's glances; the feet need a contact model of their own.

Fix, short term: widen the band (about 20°, or scale it with the head's angle from the legs so a glance's 64–84% share stays inside), raise the turn gate so only sustained turns of several degrees reach the turn-in-place acts, and add a glance detector (the head turning fast while the hands' horizontal positions about the neck stay put) that holds the smoothed estimate. Long term, in the IK-first design, a pelvis that follows a glance only twists the hips over planted feet, which is what a real body does; feet move only for twists past about 35–45° held for a moment, so glances stop costing steps.

## Frame time: is the estimator the cause?

Probably not, though the take cannot settle it. The session log's 10 s breakdowns, medians over the two sessions (10 November against this take): frame 20.5 → 24.5 ms; `post_draw`, the slot the estimator's submission and readback run in, 0.16 → 0.23 ms (+0.07); `engine_update_render` 2.1 → 4.2 ms (+2.1); `vr_wait_frame` and `drain` +1.3 ms each; `mirror` 1.6 → 0.05 ms (the capture changed). The estimator was already running 1.6 s after the session started (log line at 01:05:37.855), and `engine_update_render` was 5.5 ms in the very first window, so there is no estimator-off stretch to compare against within the take. The two takes start in different places (the 10 November take starts near (−4943, 1175, −7702), this one near (2461, 1271, −2077)), and 81 commits landed between the builds, including the shadow fit's change. The estimator's direct CPU cost is bounded by the `post_draw` slot (under a quarter of a millisecond) and its GPU cost was measured at 0.12 ms offline. The one indirect mechanism worth checking is DXVK flushing its command stream when a staging buffer still in use is mapped with no-wait every frame; that would show as GPU-wait time rather than in `post_draw`. The A/B that settles it: in one fixed scene, toggle `controllers.body.estimator.enabled` in the debug UI for a minute each way and compare the breakdowns, and add a timestamp-query pair around the pass in the game.

## The IK-first on-foot body: a design

The binding answer: idle on foot is solved IK driven by the EgoPoser estimate with the feet planted; authored poses only where IK is ill-posed; walking keeps about 20% IK on the authored walk; jump takeoff animated, airborne IK, landing animated. The design below keeps what works in the current rig (the root drive and ledger, the cockpit shift, the push-back, the head on the neck pivot, the arm remap, the physical hands) and replaces the lower body's placement.

### What the estimator gives, and in which frame

`estimator::latest` (`payload/src/controllers/body/estimator/mod.rs`) gives, in the cockpit frame as it stands when read, the global rotation of all 22 SMPL joints (pelvis, both hips, three spine joints, knees, ankles, feet, neck, collars, head, shoulders, elbows, wrists), the pelvis and upper chest headings, the estimate's age, and the posture. There is no translation: EgoPoser places the body by aligning its head joint with the tracked head. So the pelvis's position comes from forward kinematics: on a skeleton scaled to the player's calibrated proportions (the body calibration already measures them), the pelvis is the tracked head (at the neck pivot, which the camera already uses) less the chain from the pelvis to the head under the estimated spine and neck rotations. That gives a pelvis that sits behind the head when the player bends over and lower when they crouch, which the current rig infers geometrically from one spine segment.

What to take from the estimate: the pelvis's yaw (smoothed, with the glance handling above), its pitch and roll (the bend at the hips), its position via FK, the knees' flexion (the crouch depth, split between hips, knees, and ankles), and the knees' swivel as a hint. What not to take: the feet's positions or headings (above: they wander with every glance), except as the direction a step should go when one is due. The estimate is stale past half a second; with none, the pelvis falls back to the current rig's waist pivot.

### Retargeting onto Rico

Do it with positions, not rotations, so no bone's rest frame has to be known (the current solver's principle). Scale the FK pelvis's offset from the head by Rico's proportions over the player's (the arm remap already does this for the arms). Feed HumanIK: the hips effector with a position and an orientation (the FK pelvis, its yaw from the estimate), the ankle and toe effectors with the planted contacts below, and the knees as weak hints. The torso keeps the current anchored solve, but on the IK pelvis rather than the clip's, which also takes the clip's lean out (the 278 s item). An ordering hazard applies: the torso is written at the gate before HumanIK solves the lower body, so either seed the whole posture and let one full-body solve reconcile it, or hold the pelvis at its goal (high resist, no pull from the wrists) so the torso computed on it stays valid. Which of these HumanIK supports is the first thing to establish in the game (the "controlled stand" below).

The game's locomotion IK task queues hip translation, sometimes hip rotation, and ankle and toe targets every on-foot tick (`NLocoIKTask_Update`). The IK-first states must own those effectors outright, including the current reach, pull, and resist values, and release them cleanly (invalidated targets keep steering while their reach decays).

### Feet: contacts and steps

Each foot is planted (a pose in its support's frame, with the support's identity), swinging (a planned landing, an arc, a duration), or unsupported. Planted feet stay put in the support's frame however the pelvis moves. A foot releases when the pelvis carries the hip out of the leg's reach, when the pelvis's twist over the feet passes about 35–45° and has held for a few tenths of a second, when the projected centre of mass leaves the support polygon by a margin, when the root has moved more than a tolerance (the stick, a shove, a platform), or on a teleport, recenter, or mode change. One foot swings at a time, alternating, over about 0.3 s, to where the FK pelvis puts the hip plus the stance width (`stance.width_m`, toe out `stance.toe_out_deg`, which keeps the square stance's intent), probed for the ground at that x/z so slopes and steps get the right height. A missed probe leaves the foot unsupported, never planted. If both feet would need to move at once (a fast lean that turns out to be a walk), hand over to the game's walk clip instead.

### The root

Keep the grounded rig's closed-loop drive, the ledger, and the cockpit shift, but aim the drive at the floor point under the FK pelvis rather than under the neck pivot with a lean radius. A bend then produces no root motion, a step produces root motion matching the feet, and there is no catch-up event to make "more evident": the visible catch-up is the stepping foot. The lean, step-commit, drift, and settle machinery becomes unnecessary in the IK states (keep it as the fallback without an estimate, and for gamepad play, where nothing changes).

### Per state

| State | Lower body | IK weight |
|---|---|---|
| Idle on foot, standing or crouching, physical steps | IK: FK pelvis, planted feet, procedural steps | 1.0 |
| Physical walking beyond one or two steps | the game's walk clip via the step drive, as now | as the stick walk |
| Stick walk, jog, sprint | the clip, with the pelvis's orientation and the spine blended toward the IK pose; the clip's feet with the game's foot IK | about 0.2, on the pelvis's yaw, pitch, and roll and the spine; not the feet |
| Turn in place | procedural turn steps (no turn-in-place acts) | 1.0 |
| Jump takeoff | the clip until the feet leave the ground, then blend out over about 0.2 s | 0 → 1 |
| Airborne (jump, fall) | IK: the estimate's legs relaxed toward a light air pose, pelvis under the head, Rico's drawn yaw toward the head | 1.0 |
| Landing (`S_LAND_*`), hard landing, fast-fall landing | the clip; contacts re-acquired when it hands back to idle | 0, back to 1 when `S_IDLE` |
| Hand climbing | torso on the IK pelvis; feet on the face where a probe hits, hanging otherwise | 1.0 on the torso |
| Grapple swings, canopy hangs, grapple hangs | the firm ragdoll (another area), its muscle target being this IK pose | target only |
| Seats, the parachute, the wingsuit, swimming, traversals, ragdolls, get-ups | unchanged | 0 |

The 20% on the walk means blending local rotations of the pelvis and spine, never model-space positions: the current `RigPose::blend` interpolates translations independently, which shortens segments and moves endpoints mid-blend. Every hand-over between IK and a clip blends the IK weight over 0.2–0.3 s and drops the contacts it leaves.

### Pitfalls that killed the old procedural legs

The 2 November deletion followed a standing session where the legs dangled and slid, the body drifted all session, and the IK thrashed; the user judged that lower-body IK would take months and that an extra step is a graceful failure while sliding legs are not. The old rig (`git show 5d427afe^:docs/mod/body/rig.md`) was a constrained fit: each tick an optimiser searched for hips, knees, ankles, and spine that matched the head and hands, rejected fits whose residual was too large, held the last accepted fit for a grace and faded to the animation past it, inferred foot contacts inside the fit, and drove the root from the fitted stance. What went wrong, and what the new design does about it:

- Nothing told the fit where the pelvis was; it inferred it from the head alone, and a low head does not say whether the knees or the waist bent. The learned estimate now supplies the pelvis's heading and bend and the knees' flexion, which is the prior the old fit lacked.
- The feet came out of the fit, so they moved whenever the fit's solution moved. Here the feet are planted in support space and move only by an explicit step, so they cannot slide; the worst failure is an extra step.
- The fit jumped between solutions and the rejection, grace, and fade machinery made that visible as thrashing and fading. Here nothing is searched: the pelvis is FK from smoothed rotations, the feet are state, and HumanIK articulates between them.
- The root chased the fitted stance, so fit errors walked the body away ("drifted all session"). Here the root keeps the current closed loop on the ledger, aimed at the FK pelvis, with the push-back and the recenter as now.
- Native targets and stale entries fought the mod's (the hip and foot targets the locomotion IK task queues). The new states must own those effectors per channel and release them cleanly.

The memory records the user's 2 November decision as "revisit much later"; the binding answer now asks for the revisit, so the memory note about not reviving procedural legs is superseded for idle and air.

### Constraints and contacts

The design holds the body to these, in priority order when they conflict: the view, then the planted feet, then the pelvis kept within reach of the movement proxy, then the spine, then the hands. Each foot contact is stored in its support's frame (the ground, or a vehicle deck under it), so a moving support carries the foot and the contact is rebuilt in world space at render time, not at the physics tick. The mod owns the HumanIK effectors it drives per channel, the native locomotion IK task's hip and foot targets included, and releases them cleanly on a hand-over; the solve gate's effector masks are a hazard, since a mask left on by one state silently disables a target the next relies on. Authored bodies stay for actions (locomotion, jumps' takeoffs and landings, traversals). A headset and two controllers alone cannot reveal the pelvis, the knees, or the chest's heading; the learned estimate now supplies them as the body policy's prior. Two findings from this take bear on it: the square stance keeps the old footprint's height (899 s above), a live bug, and the clip's forward lean (278 s) is a reason to go procedural alongside its bladed stance.

### Stages

1. Offline, on this take: extend `rig-eval` with the FK pelvis from the replayed estimate (it already replays the estimate with `--weights`) and a contact model, and report steps per standing minute, slide (none by construction, but check the reach violations), twist distribution, and steps per glance. Targets: steps only on real body turns and room steps, a few a minute standing.
2. In the game, a controlled stand: a held pelvis goal, two planted ankle and toe goals, tracked head and wrists, and the native lower-body targets owned, to establish which HumanIK configuration holds the pelvis and the feet without the torso fighting them.
3. Idle on flat ground, then slopes and steps, then the per-state hand-overs (jump, air, landings), then the 20% walk blend.

Each stage ships enabled, with its switch and tuning in the debug UI.

## Conflicts and cross-area notes

- The reviewer's movement-aligned yaw (2044 s) conflicts with backpedalling and strafing (278 s) and with the IK-first design, where the belt should follow the player's real pelvis; the head-aligned facing plus an estimate-driven, stride-stabilised belt frame serves the same goal.
- The climb recentering (1313 s) is a translation and should only move with the player's own pulls; the surface-following climb is preferable because it moves nothing the player does not cause.
- The square stance is the sanctioned 6 November exception; the IK-first idle replaces it, and until then it needs the gating and ground-probe fixes above.
- The fast-fall landing's stance slide also touches 870.2 s and 1807 s (camera-body items in another area), and the 1806 fix (airborne IK, no torso hand-over in falls) overlaps the camera-body detachment items after landings and get-ups (297 s, 445.9 s, 597.3 s), which belong to the camera's area.
- The firm ragdoll for swings and hangs (another area) should take this IK pose as its muscle target, so both share one pose source.
