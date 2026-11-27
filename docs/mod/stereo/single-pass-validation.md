# Single-pass validation

Multipass renders correct stereo, so every single-pass change ([single-pass-stereo.md](single-pass-stereo.md)) has an oracle. The capture harness renders one held frame in four modes: the left eye alone (`left_only`), the right eye alone (`right_only`), multipass, and single-pass. The isolated eyes are the reference, each an eye-sized render through that eye's own camera; multipass checks the references, since its two dispatches should match them. A comparison reads the final image and the intermediate stages (depth, G-buffers, velocity, and the lit colour at several points) of each mode against the isolated eyes.

There are two paths. The original path writes every capture and its stages to files for the Python comparison and is the one described here. The fast comparison renders each mode once, compares on the GPU, and reruns a list of scenes against a re-injected payload in about a minute; [fast-comparison.md](../testing/fast-comparison.md) describes it, with the standing suite, the verdict thresholds, and the pinned anchor. Experiment configs are shared between the two.

## Variants: single-pass with a switch off

Scene content changes between schedules and sessions, so a before-and-after across two runs mixes a fix with the scene. An experiment can instead list variants, each forcing some `single_pass.*` switches:

```toml
[[single_pass_variants]]
name = "ssr-off"
switches = { ssr_per_eye = false }

[[single_pass_variants]]
name = "repeat"
```

After its configured modes the experiment configures single-pass once per variant and captures `samples` more frames in the same schedule. The switches travel as `[visual_overrides.single_pass]` and the capture profile resets them on the next configuration. The names are the switch table's field names, and the overridable set is `SinglePassSwitch` (every switch but the master); the Python side reads it from the table's manifest, and an unknown name fails both on load and in the payload. A variant with no switches is a same-schedule noise floor. Variant captures carry only the shader identity census unless the variant sets `diagnostics = true`. Variants need all four modes and exclude a matched treatment and a feature check.

A creation-time switch only applies once the shaders are re-created. The payload records the state the shader set was built under (`ShaderBuild`: whether single-pass and its collapse were live, and every creation-time switch), and a configuration whose state differs reloads the shaders before its warm-up. The reload is synchronous; a reload during which the configuration changed records no state, so the next configuration reloads again. The configuration fails if the record still differs, and every warm-up frame requires it to match, so no capture is taken over bytecode built for other switches. Each capture carries the record as `visual.shader_build`, and the experiment checks it against the requested values.

Each variant becomes a manifest case, `<experiment>-variant-<name>`, borrowing the default case's isolated-eye and multipass samples. The report's `single_pass_variants` lists per eye the final-image pixels more than three codes from the isolated eye over every pairing of samples (median, range, and each pair).

## Varied scenes

### Staging

A `[scene.stage]` table sends a `visual_stage` request before the first configuration (`crates/jc3vr-tooling/src/testing/stage.rs`, `tools/rendering/protocol/stage.py`). The payload teleports the player through the director's teleport, waits until the player has stood in gameplay with no teleport in flight for sixty frames, submits the spawns through the director's spawner, and lets the game run for `settle_seconds` of wall-clock time before a configuration holds the clock. The acknowledgement's landing position is reported as `stage`:

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

A spawn takes any tag set the spawn system matches (the spawner's `spawn_defs.json` lists them). Spawned entities persist, so each tag set and position is submitted once per session. A stage is refused while a configuration holds the clock, and the player stays where it left them, so a later config without a stage runs there.

### Weather

`scene.environment.weather` takes `sunny`, `rain`, or `snow`, each fired as the named event that pins it ([`WeatherController::EVENT_RAIN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/environment::WeatherController##const-EVENT_RAIN) and its siblings) followed by the instant event, and every capture checks the observed weather. The game has no overcast or fog state: weather is one severity with rain and snow ratios. A storm pinned only at the configuration barely changes the scene, so a staged scene's stage sets the hour and fires the weather before the teleport, and the weather runs for the whole settle.

### A moving camera

A `[motion]` table in place of `scene.position` and `scene.orientation` drives the camera on a periodic trajectory about the game's frozen camera, one step per frame, in every configuration (angles as `sin θ`, pitch as `sin 2θ`, translation as `sin θ`, `θ` the fraction of `period_frames`). Every capture is taken at `phase` after a full cycle, with the eight preceding frames on the trajectory, so the previous-frame matrices match across modes:

```toml
[motion]
period_frames = 120
yaw_deg = 15.0      # amplitudes, 0 to 45 degrees
pitch_deg = 5.0
forward_m = 3.0     # right_m, up_m, forward_m: -20 to 20 metres
phase = 15
```

All amplitudes at zero frame a staged location at the game's own camera. Motion captures take only the shader identity census beyond the stage images.

### Leaving a source out

`fsr_hybrid_velocity = "disabled"` in `[visual_overrides]` leaves out FSR's hybrid motion vectors in every mode. The render passes have no such override, because skipping a pass is not a clean subtraction under single-pass (a consumer still reads what it would have written); multipass is the reference instead.

### A headset-like rig

The simulated headset has symmetric, parallel eyes. `headset` in `[visual_overrides]` names a rig the frame loop substitutes for the located views in every mode (`crates/jc3vr-capture/src/testing/visual/headset.rs`): each eye's field of view, its turn from the head, and the IPD. Everything downstream sees it, including the cull and shadow-fit frusta and the compositor. The presets (`index`, `quest`, `stress`, and `beyond`) approximate each headset's view shape rather than vendor data; `stress` exaggerates asymmetry and cant, and `beyond` keeps a square eye for the Bigscreen Beyond 2e. Each keeps the tangent aspect of the eye target it is meant for. The per-eye resolution is the session's: `--eye-size WIDTHxHEIGHT` (passed through by `python -m tools.rendering.session.matched_run`, with `--resolution-scale`) seeds `vr.recommended_size_override`, and `stages = "geometry"` keeps only depth, albedo, normals, and velocity when the images grow. The first located frame logs the runtime's own fields of view and turns. Each capture records `visual.headset`, and the experiment checks each eye's recorded projection, turn, and offset against the preset (`tools/rendering/protocol/headset.py`). A rig applies to the whole experiment; `tools/rendering/experiments/headset_*.toml` are the town under each.

### Running the shipped settings

The capture profile pins or turns off FSR, the foveation, the exposure metering, the screen fade, and the player damage ("The shipped settings under single-pass" in [single-pass-collapse-defects.md](single-pass-collapse-defects.md)). `[visual_overrides]` puts each back in every mode with `fsr`, `foveation`, `auto_exposure`, `screen_fade`, and `player_damage` set to `"enabled"`. FSR's history is reset on every configuration, and each capture carries the applied exposure as `visual.exposure`. `tools/rendering/experiments/shipped_town_static.toml` and `shipped_town_rotation.toml` run the town that way.

### Coverage configs

`tools/rendering/experiments/coverage_*.toml` stage eight locations (the capital's old town, a coastal town, an airport hangar, a military harbour, a road tunnel, a mountain-top base, a meadow, and farmland) at midday and night, some at dusk and in rain and snow, the town with spawned traffic, moving cameras, and fixed-camera configs at the poses those observed carrying the census, variants, or a left-out source. `coverage_*_fixes.toml` measure individual fixes.

## The draw census

A capture can carry a census of its frame's draw calls (`crates/jc3vr-capture/src/harness/draw_census.rs`), requested by `draw_census = true` under `[diagnostics]` with `shader_identity = true`; the `*_census.toml` experiments turn it on at the coverage scenes, and a motion run's observed pose (`visual.pose.anchor`) can be reused in a fixed-pose census config. For each draw it records the pass, the render-block type, the bound vertex shader (by name, and whether it is patched) and pixel shader (by pristine bytecode hash), the entry point, the treatment (`instance_doubled`, `per_eye`, `once`, `suppressed`, or `repeated`), and the eye inside a block re-issue (`reissue_eye`) or a split pass (`split_eye`), folding identical calls into one entry. It only observes, and a multipass capture records both walks as dispatches 0 and 1, though its vertex shader names can be stale after the shader reload between modes.

`python -m tools.rendering census <capture.json> --bundle <Shaders_F.shader_bundle> --passes FIRST:LAST --totals` prints a pass range with pixel shaders named from the bundle. `--accounting --multipass <multipass capture.json>` (`tools/rendering/comparison/draw_accounting.py`) counts per pass, block type, and shader the game's calls (leaving out the collapse's repeats and the second eye of re-issues and splits, which within the routed range agrees with multipass's first dispatch), the single-pass submissions, multipass's, and the submissions beyond one per game call. Compare the prepasses as a mean over several frames, since the engine amortises its shadow cascade updates.

## Determinism

The configuration holds the clock, which freezes everything animated on it, yet the isolated eyes still repeat within a mode at up to a few thousand pixels per eye. Read every difference against the `repeat` variant and the isolated eyes' own repeats. Known sources: the global illumination solver refreshes one of its two volumes per frame (the same in every mode); the ocean band at the horizon animates on its own clock; FSR is not deterministic, so its floor is its own self-repeat; the exposure has not converged in thirty warm-up frames after a large change of scene, so the first schedule after a teleport is discarded; and a burning barrel drifts with the clock held, probably a fire light on real time. The light-propagation volume follows the live head unless the anchor is pinned, so separate runs render one scene only with the pinned anchor.

## What the comparison is expected to show

With the pinned anchor, multipass matches the isolated eyes within the repeat. Single-pass differs in a few known ways, which a contributor reads as its floor rather than as regressions.

A depth difference: two renders through transforms that agree in real arithmetic land a few floats apart, as every reprojected family does ("Exact only in real arithmetic" in [single-pass-stereo.md](single-pass-stereo.md)). The fast comparison counts a depth as differing when it is more than 64 floats from the reference (about one part in 130,000) and reports the exact count beside it as `changed`; displaced geometry lands hundreds to thousands of floats apart.

### The right eye's half of the target

The right eye's pixels lie one eye width to the right of an eye rendered on its own. Drawing eye 0's rows into both halves shows the error follows the half, not the eye: rasterizing the same vertices further right rounds the interpolated depth (one to three floats) and attributes (a code where the terrain's large patch-local coordinates cross a filter boundary). The right eye's large `changed` counts are this. No fix is expected.

### The final images

With the geometry matched, single-pass's final images still differ at a fraction of a percent of the eye. The lit colour (R11G11B10) differs mostly by one representable value at bright, high-contrast surfaces, and the anti-aliasing amplifies it: SMAA's luma edge detection flips where a neighbour moved by a code and then moves the pixel by several codes. Where the G-buffer itself differs (the foliage and other reprojected families at the meadow and the shore) everything downstream does too.

### The headset's size

The comparison runs at a real headset's eye size (3584 by 3561 per eye, a double-wide target of 7168 by 3561), but the original path writes several gigabytes per schedule there, so use the fast comparison. A sub-pixel error at the default 320 by 360 can be several pixels at the headset's size; run a candidate fix at both.
