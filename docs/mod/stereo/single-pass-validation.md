# Single-pass validation

Multipass stereo renders correct stereo, so every single-pass change ([single-pass-stereo.md](single-pass-stereo.md)) has a correctness oracle. The capture harness renders one held frame in four modes: the left eye alone (`left_only`), the right eye alone (`right_only`), multipass, and single-pass. The isolated eyes are the reference: each is an eye-sized render through that eye's own camera, which is what single-pass should reproduce. Multipass is a check on the references, since its two dispatches should match the isolated eyes. A comparison reads the final image and the intermediate stages (the depth, the G-buffers, the velocity, and the lit colour at several points of the frame) for each mode against the isolated eyes.

There are two paths. The original path writes every capture and its stages to files for the Python comparison, and is the one described below. The fast comparison renders each mode once, compares on the GPU, and reruns a list of scenes against a re-injected payload in about a minute; [fast-comparison.md](../testing/fast-comparison.md) describes it, along with the standing suite, the depth tolerance, the verdict thresholds, and the pinned anchor that makes separate runs render one scene. Experiment configs are shared between the two.

## Capturing single-pass with a switch off

Scene content at the capture views changes between schedules and sessions (the water, the vegetation, the player's pose), so a before-and-after measured across two runs mixes a fix's effect with the scene's. An experiment can instead list single-pass variants, each with some of the `single_pass.*` switches forced:

```toml
[[single_pass_variants]]
name = "ssr-off"
switches = { ssr_per_eye = false }

[[single_pass_variants]]
name = "repeat"
```

After its configured modes, the experiment configures the single-pass mode once per variant and captures `samples` more frames, in the same schedule as the default single-pass capture and the isolated eyes. The switches travel in the configuration request as `[visual_overrides.single_pass]`, beside the request's other overrides, and the capture profile resets them on the next configuration, so a variant lasts exactly as long as its captures. The names are the configuration field names of the switch table, and the overridable set is `SinglePassSwitch` (see "Configuration" in [single-pass-stereo.md](single-pass-stereo.md)): every switch except the master switch. The Python side reads the set from the table's manifest. An unknown name fails the configuration, on the Python side when the experiment loads and in the payload when it parses the request. A variant with no switches captures the default configuration again, which gives the others a same-schedule noise floor. Variant captures carry only the shader identity census of the experiment's diagnostics, which keeps them small, unless the variant sets `diagnostics = true`. Variants need all four modes configured and exclude a matched treatment and a feature check.

A switch that is a rewrite at shader creation only takes effect once the shaders are re-created. The payload records the state the shader set was last created under, `ShaderBuild`: whether single-pass and its collapse were live, and the value of every creation-time switch. A configuration whose state differs from that record reloads the shaders before its warm-up starts. The reload is synchronous (the engine re-creates every shader before `LoadShaderBundle` returns), and a reload during which the configuration changed records no state, so the next configuration reloads again. The configuration fails if the record still differs after the reload, and every warm-up frame requires it to match, so a capture is never taken over bytecode built for other switches. Each capture's metadata carries the record as `visual.shader_build` next to the effective configuration, and the experiment checks, for every single-pass capture of a run with variants, that the effective configuration holds the requested values and that the shaders were built under it.

Each variant becomes its own manifest case, named `<experiment>-variant-<name>`, which borrows the default case's isolated-eye and multipass samples, so the comparator measures it against the same references. The experiment report's `single_pass_variants` lists, for the default capture and each variant, the forced switches and, per eye, the final-image pixels that differ from the isolated eye by more than three codes, over every pairing of an isolated-eye sample with a single-pass sample (the median, the range, and each pair); the log states the same as one line per variant.

## Capturing varied scenes

To reach the shader families a fixed view never draws, an experiment can move the player somewhere else, pin the weather, put objects in front of the camera, and move the camera.

### Staging

A `[scene.stage]` table makes the experiment send a `visual_stage` request before its first configuration (`crates/jc3vr-tooling/src/testing/stage.rs`, `tools/rendering/protocol/stage.py`). The payload teleports the player through the director's teleport, which streams the destination in when it is far, waits until the player has stood in gameplay with no teleport in flight for sixty frames, submits the spawns through the director's spawner, and then lets the game run for `settle_seconds` of wall-clock time so streaming, physics, and the spawned entities come to rest before a configuration holds the clock. The acknowledgement reports where the player landed, which the experiment report carries as `stage`:

```toml
[scene.stage]
settle_seconds = 20.0

[scene.stage.location]
position = [2091.2, 1040.1, 5005.0]
yaw_deg = 270.0        # heading about world up, from +Z toward +X

[[scene.stage.spawn]]
tags = "vehicle,ground,car,civilian,vintage,mugello,sports,unarmed,unarmored,ambient,liberated,highend"
position = [2087.0, 1041.1, 5007.5]
yaw_deg = 0.0
```

A spawn takes any tag set the spawn system matches (the spawner's `spawn_defs.json` lists the shipped ones). Spawned entities persist, so the payload submits each tag set and position once per session and reports a repeat as already present; a timed explosive barrel explodes during the first settle and is gone by the next. A stage is refused while a configuration holds the clock. The player stays where the stage left them, so a config without a stage after one with it runs wherever that was.

### Weather

`scene.environment.weather` takes `sunny`, `rain`, or `snow`, each fired as the named weather event that pins it ([`WeatherController::EVENT_RAIN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/environment::WeatherController##const-EVENT_RAIN) and [`EVENT_SNOW`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/environment::WeatherController##const-EVENT_SNOW) beside the sunny one), followed by the instant event. Every capture's observed weather control must name the requested weather, and the observed severity must be clear for `sunny` and a storm for the other two. The game exposes no overcast or fog state of its own: the weather is one severity scalar with rain and snow ratios, and the named events are the states it can hold. Pinned at the configuration alone, a storm never runs with the clock and barely changes the scene, so a staged scene's stage also carries the environment: the payload sets the hour and fires the weather event before the teleport, without holding the day cycle, and the weather runs for the whole settle before the configuration pins it again. The stage saves nothing to restore; the configuration's restore returns the weather to the ambient system, and the hour stays the staged one.

### A moving camera

A `[motion]` table in place of `scene.position` and `scene.orientation` drives the camera along the payload's periodic trajectory about the game's frozen camera, one step per rendered frame, in every configuration: the angles swing as `sin θ` (pitch as `sin 2θ`) and the camera-local translation as `sin θ`, `θ` being the frame's fraction of `period_frames`. Every capture is taken at `phase`, after a full cycle and with the eight frames before the phase rendered on the trajectory, so the previous-frame matrices the temporal effects read are the same trajectory frames in every mode. The clock is held, so only the camera moves:

```toml
[motion]
period_frames = 120
yaw_deg = 15.0      # amplitudes, 0 to 45 degrees
pitch_deg = 5.0
forward_m = 3.0     # right_m, up_m, forward_m: -20 to 20 metres
phase = 15
```

All amplitudes at zero give a static camera at the game's own camera, which is the simplest way to frame a staged location without knowing its terrain height. A motion experiment's captures take no diagnostics beyond the shader identity census, but every capture still writes the stage images the comparison reads, and its variants work as in a fixed-pose experiment. To name a scene's draws, a fixed-pose config at the camera a motion run observed (the capture metadata's `visual.pose.anchor`) can carry `draw_census` ([single-pass-draw-calls.md](single-pass-draw-calls.md)).

### Leaving a source out

To tell whether a difference comes from one producer, `[visual_overrides]` takes `fsr_hybrid_velocity = "disabled"`, which leaves out FSR's hybrid motion vectors in every mode, isolated eyes included. The capture profile resets it on the next configuration. The render passes themselves have no such override: leaving one out is not a clean subtraction under single-pass (a consumer still reads what the skipped pass would have written), so multipass, run alongside, is the comparison's reference. The motion blur never runs in VR, so the final images do not read the velocity buffer; the buffer itself is captured in every mode as the `velocity` stage.

### A headset-like rig

The harness's simulated headset has symmetric, parallel eyes, which leaves every path that only holds for those eyes unexercised. `headset` in `[visual_overrides]` names a rig that the frame loop substitutes for the located views in every mode, where it reads them (`crates/jc3vr-capture/src/testing/visual/headset.rs`): each eye's field of view, its turn from the located head, and the distance between the eyes. Everything downstream sees the rig, from the per-eye projections and offsets to the cull and shadow-fit frusta and the single-pass rows, and the compositor is handed the same views. The presets approximate the shape of each headset's views; they are not vendor data:

| `headset` | Left eye's field of view (left, right, up, down) | Turn of each eye | IPD |
|---|---|---|---|
| `"index"` | −54°, 51°, 55.8°, −55.8° | 5° outward | 64 mm |
| `"quest"` | −50°, 43°, 48°, −52° | none | 63 mm |
| `"stress"` | −60°, 35°, 49°, −58° | 12° outward, pitched 1° up (left) and down (right), rolled 1° outward | 70 mm |
| `"beyond"` | −51°, 45°, 48.2°, −48.2° | none (no measured cant is available; parallel displays assumed) | 63 mm |

The right eye mirrors the left. Each preset keeps the tangent aspect of the eye target it is meant for, so its pixels stay square there: the harness's default 320 by 360 for the first three, and a square eye for `beyond`, approximating a Bigscreen Beyond 2e. The per-eye resolution is the session's, never the rig's. A session launched with `--eye-size WIDTHxHEIGHT` (`python -m tools.rendering.session.matched_run` passes it, and `--resolution-scale`, through) seeds `vr.recommended_size_override` in place of the simulated headset's recommended size, so the harness can capture at a real headset's per-eye size; the width is still rounded up to the tile the payload aligns every eye to. The stage images grow with the eye, so `stages = "geometry"` in `[visual_overrides]` keeps only the depth, the albedo and normal G-buffers, and the velocity. The first frame that locates the views logs the runtime's own field of view and turn from the head for each eye, so a session on a real headset records what a preset can only approximate. Each capture records the rig it was rendered under as `visual.headset`, and the experiment checks that it is the requested preset and that each eye's recorded projection, turn from the centre camera, and offset carry it (`tools/rendering/protocol/headset.py`). The full-camera freeze holds the eye parameters it latched under the rig in force, and latches them again when a configuration names another. A rig applies to every mode of an experiment, so comparing rigs takes one experiment per rig; `tools/rendering/experiments/headset_*.toml` are the town at midday under each.

### Running the shipped settings

The capture profile pins or turns off what a player runs with: FSR, the foveation, the engine's exposure metering and its histogram, the screen fade, and the player-damage overlay ("The shipped settings under single-pass" in [single-pass-collapse-defects.md](single-pass-collapse-defects.md) has what each does under the collapse). `[visual_overrides]` puts each back at the mod's shipped value in every mode with `fsr`, `foveation`, `auto_exposure`, `screen_fade`, and `player_damage` set to `"enabled"`. FSR's history is reset on every configuration, so every mode accumulates it from its own warm-up. Every capture's metadata carries the exposure the post chain applied as `visual.exposure`, so captures with the engine's metering can be checked for one shared exposure. `tools/rendering/experiments/shipped_town_static.toml` and `shipped_town_rotation.toml` run the town with every shipped setting put back, with a repeat and each fix's switch off as variants.

### Coverage configs

The coverage configs are `tools/rendering/experiments/coverage_*.toml`: eight staged locations (the capital's old town, a coastal town, a hangar at a military airport, a military harbour, a road tunnel, a mountain-top base, a meadow below a wooded hill, and farmland) at midday and at night, two at dusk and in rain and snow, the town with a spawned car and pedestrian, a rotating and a translating camera at the town, and fixed-camera configs at the poses those runs observed, carrying the draw census, switch-off variants, or a left-out source for attribution. The `coverage_*_fixes.toml` configs measure individual fixes at their poses.

## Determinism

The configuration holds the clock, which freezes everything the game animates on it: pedestrians, traffic, wind turbines, and rain particles stand still, and the isolated eyes' captures repeat within a mode at up to a few thousand pixels per eye, often a few hundred, and none in some scenes. Read every scene's single-pass difference against the same-schedule floor the `repeat` variant gives and against the isolated eyes' own repeats. Several sources keep the floor above zero:

- The global illumination solver refreshes one of its two volumes per frame, so consecutive frames differ by a step at a few hundred thousand pixels of the lit colour at the headset's size. It is the same in every mode, and with the pass left out the repeats fall to a few pixels.
- The ocean band at the horizon differs between any two snapshots taken at different times, in every mode, because the ocean animates on its own clock.
- FSR is not deterministic from capture to capture, so its noise floor is its own self-repeat.
- The first capture after a large change of scene has not converged its exposure in thirty warm-up frames, so the first schedule after a teleport is discarded.
- The capital's old town with a burning explosive barrel drifts between captures with the clock held, so that scene cannot be compared pixel for pixel. A fire's light animating on real time is the likely source; it is unverified.

The light-propagation volume follows the live head unless the anchor is pinned, and the frozen camera starts wherever the live camera was, so separate runs render the same scene only with the pinned anchor ("The pinned anchor" in [fast-comparison.md](../testing/fast-comparison.md)).

## What the comparison is expected to show

With the pinned anchor, multipass matches the isolated eyes within the repeat. The screen-space reflection march samples the previous frame's scene, and in multipass the previous image is the other eye's, so multipass is the odd one out there; its effect is within the repeat once the anchor is pinned. Single-pass and the isolated eyes each read their own previous frame.

Single-pass is expected to differ from the isolated eyes in a few known ways, which a contributor reads as the floor of a single-pass comparison rather than as regressions.

### What a depth difference is

Two renders of one surface through transforms that agree in real arithmetic but round differently land a few floats apart: every reprojected family against the isolated eye's own matrices does ("Exact only in real arithmetic" in [single-pass-stereo.md](single-pass-stereo.md)). An exact rule counts those in the millions at the headset's size and hides displaced geometry among them, so the fast comparison counts a depth as differing when it is more than 64 floats from the reference (about one part in 130,000, 0.08 mm at ten metres) and reports the exact count beside it as `changed`. Displaced geometry lands hundreds to thousands of floats apart. Multipass against the isolated eyes stays within a few dozen pixels beyond the tolerance, so the references agree to the bit almost everywhere.

### The right eye's half of the target

In the double-wide target the right eye's pixels lie one eye width to the right of an eye rendered on its own. Drawing eye 0's rows into both halves (every `cb13` slot, `M_eye`, and the terrain detail's rows pinned to eye 0) puts the left eye's image one eye width further right in the right half; against the isolated left eye, the right half then differs as the right eye normally differs from the isolated right eye, while the left half matches. The error follows the half, not the eye: rasterizing the same vertices one eye width further right rounds the interpolated depth and attributes differently (one to three floats of depth, and a code of albedo or normals where the terrain's texture coordinates, interpolated from patch-local positions hundreds of metres in size, cross a filter boundary). The right eye's large `changed` depth counts are this rounding. No fix is expected.

### What the final images differ by

With the geometry matched, single-pass's final images still differ from the isolated eyes at a fraction of a percent of the eye, against a repeat floor several times smaller. At the town the G-buffer is bit for bit the isolated eye's over nearly all of the left eye, and almost all of the lit colour's and the final image's differences lie inside that identical region, as a speckle a few codes deep over the sunlit ground:

- At the lit colour, which is R11G11B10, the differing pixels nearly all differ by exactly one representable value. A one-step change at bright, high-contrast sand is one or two codes after the tone map.
- The anti-aliasing turns those codes into the speckle. SMAA's edge detection, a luma threshold over a high-frequency texture, flips where a neighbour moved by a code, and the blend it then applies moves the pixel by 4 to 8 codes, so many final pixels over three codes had a bit-identical input to SMAA. Every comparison has this amplifier, the repeats included.
- The screen-space ambient occlusion was most of the lit difference until its sampling was made the same in every mode ("The ambient occlusion's sampling, the same in every mode" in [single-pass-collapse-defects.md](single-pass-collapse-defects.md)). What remains of it follows its inputs: the linear depth where the reprojected families' depth differs, and the occlusion volumes' target.
- The right eye's half carries the rest of the right eye's difference.
- At the meadow and the shore the G-buffer itself differs at the foliage and the other reprojected families, so everything downstream does too.

What is left of the lit colour's one-step differences once those are accounted for is not attributed.

### The headset's size

The comparison runs at a real headset's eye size (3584 by 3561 per eye, the Bigscreen Beyond 2e's, so a double-wide target of 7168 by 3561) without hitting a texture-size or memory limit, but the original path writes several gigabytes per schedule there even with `stages = "geometry"`; use the fast comparison. Defects can be invisible at the harness's default 320 by 360 and visible at the headset's size, because a sub-pixel error at the small size becomes several pixels at the large one; run a candidate fix at both.
