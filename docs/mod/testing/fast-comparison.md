# Fast comparison

The capture harness (`tools/rendering/`, with the payload side in `payload/src/testing/` and `payload/src/screenshot/harness/`) compares single-pass stereo against the isolated eyes and multipass on a frozen scene. Its original path renders each mode three times, reads every capture and its stages back to files, and compares them in Python afterwards. The fast comparison renders each mode once (plus one repeat frame), keeps the images on the GPU, compares them there, and reads back only a summary. It is the inner loop for single-pass work: build, re-inject, and rerun a list of scenes in about a minute against a live session, with no relaunch, no world load, and no checks beyond the build. The original path stays for anything that needs the full stage set or the diagnostics, and its reports read the same captures as before.

## The fast loop

Launch a session once. The guarded launcher still requires the five check results by default; `--build-only` requires only `build.exit`, for a session that only serves the inner loop. Without a config it launches and returns once the game is in gameplay:

```sh
python3 -m tools.rendering.session.matched_run N --gpu 10de:2b85 --prior M --unmatched --build-only
python3 -m tools.rendering.session.matched_run N --gpu 10de:2b85 --prior M --unmatched --build-only --resolution-scale 1.0 --eye-size 3584x3561
```

Then, after each change:

```sh
python3 -m tools.rendering iterate --session N --only town tunnel
python3 -m tools.rendering iterate --session N --configs tools/rendering/experiments/coverage_town_midday.toml
python3 -m tools.rendering iterate --session N                      # the whole standing suite
```

`iterate` builds the payload and the injector incrementally (`cargo xwin build ... -p jc3vr_payload -p jc3vr_injector`), ejects the loaded payload and injects the new one (see "Re-injection"), and runs the scenes. `fast` does the last step alone against whatever payload is loaded (`--runner` takes a `runner.json` in place of `--session`). Both print one line per scene, test, reference, and eye with each plane's differing pixel count over its noise floor, write `fast-report.json` (every comparison, the heat maps, the floors, and the time each step took) under `target/harness-probe/fast/<stamp>/` or `--output`, and exit 0 when nothing fails, 1 when something does, and 2 when a scene could not be compared. Run the full checks (`cargo +nightly fmt --all -- --check`, `./scripts/xwin_clippy.sh`, `./scripts/xwin_test.sh`, and the Python tests) once before committing.

Options: `--planes` picks the planes (the final image is always compared), `--velocity` adds the velocity buffer, `--dump never|on_failure|always` and `--no-crop` control the dumps, `--baseline <fast-report.json>` judges regressions against an earlier run, `--restage` stages afresh, `--full-cycles` runs a motion that never moves the camera as configured, `--no-repeat` skips the noise-floor repeats, and `--warmup-frames` and `--settle-seconds` override the configs' warm-up and settle.

## The standing suite

`tools/rendering/suites/standing.toml` lists the regression matrix: the town at midday, the rotating and translating cameras there, the shipped settings static and rotating, the town under each headset rig (`index`, `quest`, `stress`, and `beyond`), the tunnel, the meadow, the capital at night, the shore lake, and the cliff at dusk. A suite entry names a config under `tools/rendering/experiments/` and may add `headset = "<rig>"` (the `[visual_overrides] headset` of every mode) or a `[scene.stage]` table for a fixed-camera config that carries none; the two production views are staged below their cameras, as the capture batches do. Entries at one location with one environment are listed together, so the stage runs once for them. The suite runs at the session's eye size: a session launched with `--resolution-scale 1.0 --eye-size 3584x3561` runs it at the headset's.

## Snapshots

A `snapshot` request (`payload/src/screenshot/harness/snapshot/`, `tools/rendering/protocol/snapshot.py`) keeps the next eligible frame's planes as GPU textures under a label:

```toml
command = "snapshot"
id = "f1234abcd_single_pass_a"

[motion_capture]   # only under a motion configuration
phase = 15

[snapshot]
label = "single_pass_a"
planes = ["final", "depth", "albedo", "normals", "velocity"]
keep = true                       # keep the textures for later snapshots to compare with
compare = ["left_only_a", "right_only_a", "multipass_a"]
pixel_threshold = 3.0             # the final image's per-pixel threshold, in 8-bit codes
fail_fraction = 0.001             # the failure limits the dump policy uses
fail_mean = 0.25
dump = "on_failure"               # never (the default), on_failure, or always
dump_crop = true
```

The frame is chosen as a screenshot's is: in gameplay, with the VR session rendering and the mode's eyes copied fresh, once the visual configuration is ready, and under a motion configuration at the requested phase after a full cycle. The final image is copied from the same source a screenshot reads (the collapsed back buffer, the isolated eye's, or both eye captures in multipass), and the geometry planes from the stage hooks at the end of the G-buffer range, where the screenshot's stages are read: the depth buffer, `gbuffer0` (albedo), `gbuffer1` (normals), and the velocity buffer. Each plane is copied with one GPU copy into a texture of its format family, so taking a snapshot never stalls. A single-pass plane holds both eyes side by side and is split at the eye width.

The snapshot is then compared with each kept snapshot `compare` names, plane by plane and eye by eye where both have the eye, by `snapshot_diff_cs` (`payload/src/shaders/snapshot_diff_cs.hlsl`): one thread group per cell of a 32 by 32 heat map over the eye, each reducing its cell to the differing pixel count, the sum of the per-pixel mean channel difference, the largest channel difference, the differing pixels' bounding box, and the count of pixels whose stored values change at all. The cells of every comparison are read back in one copy (about 32 KB a comparison) and totalled on the CPU (`summary.rs`). The acknowledgement, `captures/<id>.json`, lists each comparison's reference, plane, eye, pixel count, differing count, changed count, mean difference, largest difference, bounding box, and heat map, with how many frames the snapshot waited and how long the copies, comparisons, and readback took on the render thread. A snapshot that is not kept is released once compared, and `snapshot_release` releases every kept one; the fast runner sends it at the end of each scene, and an eject releases them too. The run 296 town at 3584 by 3561 per eye took 3 to 7 ms of render-thread time per snapshot, its comparisons included.

### What counts as differing

The rules are those of the Python comparison, so the counts are comparable:

| Plane | Read as | A pixel differs when | Mean |
|---|---|---|---|
| Final image | 8-bit codes, RGB | its largest channel difference exceeds three codes (`pixel_threshold`) | over R, G, and B (`comparison/images.py`) |
| Albedo, velocity (8-bit) | 8-bit codes, RGBA | any channel's code changes | over all four channels |
| Normals (10-bit) | integers, RGBA | any channel changes | over all four channels |
| An R11G11B10 plane | floats, RGB | any channel's bits change | over R, G, and B |
| Depth | 32-bit float and 8-bit stencil | the two depths are more than `depth_ulps` representable floats apart (64 in the fast comparison, `--depth-ulps`; zero, any changed bit, by default in a request and in `comparison/stages.py`'s exact changes), or the stencil changes | of the depth |

Every comparison also reports `changed`, the pixels whose stored values change at all; for the depth under a tolerance that includes the changes the tolerance absorbs, and for every other plane it equals the differing count. The tolerance exists because two renders of one surface through transforms that agree in real arithmetic but round differently (single-pass's reprojected families against an isolated eye's own matrices, and the right eye's half of the double-wide target) land a few to a few dozen floats apart over the whole surface, which an exact rule counts in the millions at the headset's size; displaced geometry lands hundreds to thousands apart. See "Comparing at the headset's size" in [single-pass-stereo.md](../stereo/single-pass-stereo.md). A non-finite difference counts the pixel but stays out of the sums. The final image's pass or fail uses the Python comparison's thresholds (at most 0.1 % of the pixels differing and a mean of at most 0.25); the stages have no threshold in either comparison, and the fast comparison applies the same 0.1 % to them for its verdict.

### Verdicts

A scene's snapshots are taken in a fixed order (`tools/rendering/fast/plan.py`): the isolated left eye, the isolated right eye, and multipass are kept as references, each mode takes a second snapshot compared with its first, and single-pass and each variant are compared with every reference. The repeat's counts are the same-schedule noise floor for that mode, plane, and eye; a row's floor is the larger of its two modes'. A row passes within the thresholds above, counts as noise when it exceeds them by no more than twice the floor plus 0.02 % of the pixels (`--noise-factor`), and fails otherwise. Against a `--baseline`, only a row whose count grew by more than that allowance fails, as a regression. A variant that forces no switch renders the single-pass mode again, which the repeat already does, so the fast comparison skips it.

### Dumps

With `dump = "on_failure"` the comparisons over the failure limits are dumped, and with `always` every one. The reference's and the test's pixels are read back cropped to the bounding box grown by 16 pixels (the whole eye with `dump_crop = false`) and written by a writer thread into `captures/<id>.dump/`: the final image as PNG, a stage as its stored bytes compressed with gzip (`<reference>.<plane>.eye<N>.reference.raw.gz` and `.test.raw.gz`). The comparison's `dump` record in the acknowledgement names the files, the crop, and the DXGI format and pixel size to decode a raw dump with (`comparison/stages.py`'s `decode_raw` reads it after `gzip.decompress`). The acknowledgement is written after its dumps, so a dump never trails its record. A full-eye dump at the headset size is about 50 MB per plane and eye compressed; the crop is the default for that reason.

## A still camera holds a fixed pose

Most coverage configs carry a `[motion]` table with every amplitude zero, which holds the game's own camera without knowing the location's terrain height. Under the motion, every capture waits for its phase after a full cycle, 120 frames at the configs' period. The camera never moves, so the fast comparison holds the same camera with a fixed pose instead (the anchor with no offset): each snapshot is taken on the first frame after the warm-up, and its repeat on the next frame. A moving camera runs its motion as configured, cycles and all; `--full-cycles` runs a still one that way too.

Every scene still begins with an anchor configuration, as the original path does, though a still camera needs no anchor to aim at. Without it, the first configuration after the previous scene's restoration rendered differently from those after it: in run 297 both of the isolated left eye's snapshots from it differed from multipass at 114,906 of 115,200 pixels while agreeing with each other, and with the anchor first they differed at 125. Why is unmeasured.

## Reusing the stage

A staged scene's teleport and settle (20 seconds in the configs, on top of the streaming) is paid once per location. The harness records the last stage it sent in the session's configuration directory (`last-stage.json`, `tools/rendering/session/stage_record.py`): the location, the spawns, and the environment. A stage that matches the record is sent with `reuse_within_m = 5.0`, and the payload acknowledges it at once, with the environment set again but no teleport, spawns, or settle, when the player still stands within that distance of the location; otherwise it stages afresh (`payload/src/testing/stage.rs`). The acknowledgement says which it did (`reused`). The original path (`python3 -m tools.rendering run`) reuses a stage the same way, and both take `--restage` to stage afresh.

## Re-injection

`iterate` re-injects through the injector's own `--reload` (`injector/src/main.rs`), sent into the game's container the way `scripts/proton_run.sh` sends it, over the session's private bus (`tools/rendering/session/reinject.py`). The injector asks the payload to eject, waits for its image to leave the process, copies the new build to the loaded name, injects it, and runs it; the harness then waits for the new payload's session log to report gameplay. Right after an ejection, the copy of the new build over the loaded name can fail with a sharing violation for tens of seconds (in run 296 it outlasted a 30-second retry in one injector and succeeded at once in the next), which looks like Wine keeping the ejected image's file open until the injector that ejected it exits. The injector retries the copy for two seconds, and the harness then runs a fresh injector, which finds nothing loaded and injects. The new payload starts its OpenXR session, installs its hooks, and patches the shaders on its own; the next configuration's readiness checks (the shaders re-created under the requested state, the XR frame identity, and the rest) hold its first snapshot until they all pass.

In run 295, three runs of the town at 320 by 360 per eye on the freshly launched payload and three after a re-injection gave the same ranges: single-pass against the isolated left eye at 661 to 1,528 final-image pixels before and 897 to 2,089 after, against the right eye 860 to 2,422 and 1,654 to 2,009, and multipass against the isolated eyes 101 to 634 and 125 to 558. The geometry planes were bimodal in both, an eye's albedo differing at about 30 pixels or at 2,000 to 9,000, as the terrain patches' detail state is documented to do from configuration to configuration ("Capturing varied scenes" in [single-pass-stereo.md](../stereo/single-pass-stereo.md)).

Two things about the eject itself:

- The game's video memory grows by about 3 to 4 GB with each eject and re-injection (run 310: 10 GB after the launch, 14 GB after the first re-injection, 20 GB after the third, 24 GB after the fourth, and 32 GB after the fifth and a few runs with dumps). At the card's 32 GB the frame time rose to about 540 ms, almost all of it waiting, and a scene took three times as long. A session that has been re-injected several times needs a relaunch; check `nvidia-smi` when the cycle slows down. What leaks is not identified.
- It takes about fifteen seconds under the harness. The human-hull participant ([human-hull-restoration.md](../body/human-hull-restoration.md)) waits out its bounded restoration, which never completes there (`clearance=PendingBody`), with the frames slowed to about 115 ms meanwhile, and then commits with the narrowed hulls left in place. That is a state the re-injected payload inherits.
- A payload that has run the shader identity census (`shader_identity = true`, in the diagnostics of every coverage config run through the original path) cannot be ejected: the census's pixel-shader detour pins the image. The eject then tears the payload down and stops at the hook removal, which leaves the game without the mod until it restarts. The payload logs the pin when the detour is first enabled, and `iterate` reads that line and refuses to re-inject, asking for a relaunch instead. The fast comparison never arms the census.

## Session pacing

Monado's null compositor paces the session's frames, and every harness wait counts frames. It ran at 30 fps; a session now takes `--compositor-fps` (120 by default) from the launcher. The game then runs as fast as it renders: at 320 by 360 per eye about 90 fps with an isolated eye and 43 fps in multipass and single-pass, and at 3584 by 3561 about 40 fps in every mode.

## Where the time went

Measured on the town at midday (a still motion, four modes and the repeat variant, three samples each) before this path existed:

- At 320 by 360 per eye (run 292), a scene took 107 to 145 seconds: 23 seconds of stage (the 20-second settle and the teleport), 2 to 4 seconds per configuration, and 4.2 to 4.8 seconds per capture for 15 to 24 captures. A capture was one 120-frame motion cycle at the compositor's 30 fps (4 seconds), plus up to a second for the payload to notice the request (it looked for the request file once a second), so the frame counts and the fixed waits were nearly all of it; the Python comparison took 3 seconds.
- At 3584 by 3561 (run 293), the same scene took 5 minutes 26 seconds: the 23-second stage, 15 captures of 7.4 seconds (an isolated eye) and 10.7 to 11.8 seconds (both eyes), and 2 minutes 23 seconds of Python comparison over nine cross pairs per mode of 12.8 million pixels. A capture read every stage back with a synchronous map on the render thread and handed about 100 MB per plane to the writer, so its readback and writes pushed it past one and two more motion cycles.
- A code change cost the full checks, a relaunch, and a world load, since nothing re-injected the payload under the harness.

What the fast path does about each: the payload looks for requests every frame in a harness session and the harness polls for acknowledgements every 5 ms; the compositor paces at 120 fps; a still camera is a fixed pose, so a snapshot waits for the warm-up and its repeat for one frame; the comparison runs on the GPU and reads back kilobytes; nothing is written unless a dump is asked for; the stage is reused; and the payload is re-injected.

## Cycle times

Measured in runs 294 to 297 on the town at midday unless named otherwise:

| | Before | After |
|---|---|---|
| One scene at 320 by 360, stage reused | 107 s (with the stage) | 6.7 s |
| One scene at 3584 by 3561, stage reused | 5 min 26 s (with the stage) | 6.9 s |
| A first visit to a location (teleport and 20-second settle) | 23 s | 24 to 28 s |
| The GPU comparison per snapshot, at 3584 by 3561 | 2 min 23 s of Python per scene | 3 to 7 ms on the render thread |
| Build, re-inject, and rerun one scene at 3584 by 3561 | 10 to 15 minutes of checks, a relaunch, and a world load | 34 to 38 s |
| The standing suite at 3584 by 3561 | about an hour | 5 min 20 s |
| The standing suite at 320 by 360 | about 25 minutes | 5 min 15 s |

Of the rerun's 34 to 38 seconds, the incremental build takes 11.5, the re-injection 16 to 18 (the human-hull wait at the eject and a second injector run, see "Re-injection"), and the scene 6 to 7. Of the suite's 5 minutes 20 seconds at the headset size, the five stages (the tunnel, the meadow, the capital, the shore, and the cliff) take 127 seconds and the three moving cameras (the town rotating and translating, and the shipped settings rotating) 109, whose every snapshot waits out a 120-frame cycle at about 40 fps; the other nine scenes take 7 to 10.5 seconds each. The suite takes about as long at either size: the comparisons cost milliseconds, and the frame rate that the warm-ups, the single-pass mode's shader reload, and the cycles wait on is about 40 fps at both in the paired modes. The "before" suite figures are the per-scene times multiplied out, not runs. `--settle-seconds` shortens the settles, at the risk of comparing a world still streaming in, and `--only` runs a subset.
