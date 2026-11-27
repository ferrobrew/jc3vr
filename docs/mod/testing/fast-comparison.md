# Fast comparison

The capture harness (`tools/rendering/`, with the payload side in `crates/jc3vr-tooling/src/testing/` and `crates/jc3vr-capture/src/harness/`) compares single-pass stereo against the isolated eyes and multipass on a frozen scene. Its original path renders each mode several times, reads every capture and its stages back to files, and compares them in Python. The fast comparison renders each mode once plus a repeat, keeps the images on the GPU, compares them there, and reads back only a summary. It is the inner loop for single-pass work: build, re-inject, and rerun a list of scenes against a live session in under a minute, with no relaunch and no world load. The original path stays for anything that needs the full stage set or the diagnostics.

The render hooks reach the payload side only through the probe table the capture crate registers at startup (the render crate's `probes`, filled by `harness/register_probes.rs`). The hooks call the table at generic events (the pipeline points, the block and pass scopes, the game's draws, and the stereo path's submissions), and the payload side decides which diagnostics each event feeds, so a new experiment widens the harness rather than the render hooks. Until the first capture, snapshot, or visual-control request arrives (a benchmark request arms only the probes the benchmark reads), the hooks skip the table after one relaxed load of the arming flag (`probes/arming.rs`). The shader tags, which a later capture reads, are the exception and run whenever a table is registered.

## The fast loop

Launch a session once. The guarded launcher requires the five check results by default; `--build-only` requires only `build.exit`, for a session that only serves the inner loop. Without a config it launches and returns once the game is in gameplay:

```sh
python3 -m tools.rendering.session.matched_run N --gpu <vendor:device> --prior M --unmatched --build-only
python3 -m tools.rendering.session.matched_run N --gpu <vendor:device> --prior M --unmatched --build-only --resolution-scale 1.0 --eye-size 3584x3561
```

Then, after each change:

```sh
python3 -m tools.rendering iterate --session N --only town tunnel
python3 -m tools.rendering iterate --session N --configs tools/rendering/experiments/coverage_town_midday.toml
python3 -m tools.rendering iterate --session N                      # the whole standing suite
```

`iterate` builds the payload and the injector incrementally, re-injects (see "Re-injection"), and runs the scenes. `fast` does the last step alone against whatever payload is loaded. Both print one line per scene, test, reference, and eye with each plane's differing pixel count over its noise floor, write `fast-report.json` (every comparison, the heat maps, the floors, and the step timings) under `target/harness-probe/fast/<stamp>/`, and exit 0 when nothing fails, 1 when something does, and 2 when a scene could not be compared. `python3 -m tools.rendering fast --help` lists the options (`tools/rendering/fast/cli.py`). Run the full checks once before committing.

Every harness build, the session launch's included, enables the payload's `capture-rewrites` feature. It compiles in the shader rewrites only the committed experiments request, which the shipped payload leaves out; a payload built without it refuses those experiments' requests with an error naming the feature.

## The standing suite

`tools/rendering/suites/standing.toml` lists the regression matrix of scenes. An entry names a config under `tools/rendering/experiments/` and may add `headset = "<rig>"` (the `[visual_overrides] headset` of every mode) or a `[scene.stage]` table for a fixed-camera config that carries none. Entries at one location with one environment are listed together, so the stage runs once for them. The suite runs at the session's eye size.

## Snapshots

A `snapshot` request (`crates/jc3vr-capture/src/harness/snapshot/`, `tools/rendering/protocol/snapshot.py`) keeps the next eligible frame's planes as GPU textures under a label:

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
save = false                      # write every plane whole, compared or not
jitter_phase = 0                  # at a still camera, wait for this FSR jitter offset (optional)
```

The frame is chosen as a screenshot's is: in gameplay, with the VR session rendering and the mode's eyes copied fresh, once the visual configuration is ready, and under a motion configuration at the requested phase after a full cycle. The final image comes from the source a screenshot reads (the collapsed back buffer, the isolated eye's, or both eye captures in multipass), and the geometry planes (depth, `gbuffer0` albedo, `gbuffer1` normals, and velocity) from the stage hooks at the end of the G-buffer range. Each plane is one GPU copy into a texture of its format family, so a snapshot never stalls. A single-pass plane holds both eyes side by side and is split at the eye width.

Beyond the geometry planes (the plane names are in `protocol/snapshot.py`):

- The scene colour at five points, each an R11G11B10 float plane (`post_resolve`, `pre_atmosphere`, `post_atmosphere`, `post_sun`, and `main_color`), and the tone-mapped image around the anti-aliasing (`pre_aa`, `post_aa`). A float plane changes bits at a large share of its pixels between any two renders, so read them from dumps with a tolerance.
- FSR's inputs, taken right after the velocity decode for each eye FSR resolves: `fsr_motion` (the decoded motion vectors, in UV before FSR's motion-vector scale) and `fsr_depth`. Each is one eye's texture in every mode, and they exist only where FSR runs. Every acknowledgement also carries `fsr`, one `fsr::DispatchRecord` per eye FSR resolved on the frame, which `python3 -m tools.rendering fsr-motion` checks the saved planes against ([fsr-measurements.md](../rendering/fsr-measurements.md#verifying-the-motion-vectors)).
- The ambient occlusion: `ssao` (the scene target the composite multiplies the occlusion into), `ssao_volumes`, and `ssao_step0` to `ssao_step7`, the target each draw inside the SSAO block leaves. A step plane may be smaller than the eye and is compared at its own size.

The snapshot is compared with each kept snapshot `compare` names, plane by plane and eye by eye, by `snapshot_diff_cs` (`crates/jc3vr-capture/shaders/snapshot_diff_cs.hlsl`): one thread group per cell of a 32 by 32 heat map, each reducing its cell to the differing pixel count, the summed mean channel difference, the largest channel difference, the bounding box, and the count of pixels whose stored values change at all. The cells are read back in one copy and totalled on the CPU (`summary.rs`). The acknowledgement, `captures/<id>.json`, lists every comparison with how long the copies, comparisons, and readback took. A snapshot that is not kept is released once compared, and `snapshot_release` (sent at the end of each scene) and an eject release the kept ones.

### FSR's jitter in a snapshot

While FSR runs, every frame's projections carry the next offset of its jitter sequence ([fsr.md, "Sub-pixel jitter"](../rendering/fsr.md#sub-pixel-jitter)), and two configurations whose snapshots land on different offsets differ at every edge. The sequence restarts with each configuration's FSR history reset, but the frames between the reset and the snapshot vary with the readiness checks, so a still camera's snapshot names the offset it wants with `jitter_phase` and waits for it. The fast comparison asks a still camera's first snapshot for index 0 (or `--jitter-phase K`) and its repeat for the next index, so a still FSR scene's noise floor is the difference between two adjacent offsets, not the frame-to-frame noise. A configuration without FSR does not send the field, and with `fsr.jitter` off the payload ignores it. A moving camera's snapshot is taken at its motion phase; the sequence restarts when the motion begins, and the configs' motion period is a whole number of sequences, so a phase falls on the same offset in every configuration.

### What counts as differing

The rules are the Python comparison's, so the counts are comparable:

| Plane | Read as | A pixel differs when | Mean |
|---|---|---|---|
| Final image | 8-bit codes, RGB | its largest channel difference exceeds `pixel_threshold` | over R, G, and B (`comparison/images.py`) |
| Albedo, velocity (8-bit) | 8-bit codes, RGBA | any channel's code changes | over all four channels |
| Normals (10-bit) | integers, RGBA | any channel changes | over all four channels |
| An R11G11B10 plane | floats, RGB | any channel's bits change | over R, G, and B |
| Depth | 32-bit float and 8-bit stencil | the depths are more than `depth_ulps` representable floats apart (`--depth-ulps` in the fast comparison; zero, any changed bit, by default in a request and in `comparison/stages.py`), or the stencil changes | of the depth |

Every comparison also reports `changed`, the pixels whose stored values change at all, which for the depth includes what the tolerance absorbs. The tolerance exists because two renders of one surface through transforms that agree in real arithmetic but round differently (single-pass's reprojected families against an isolated eye's own matrices) land a few to a few dozen floats apart over the whole surface, which an exact rule counts in the millions at the headset's size; displaced geometry lands hundreds to thousands apart. A non-finite difference counts the pixel but stays out of the sums. The final image passes with at most 0.1 % of its pixels differing and a mean of at most 0.25, the Python comparison's thresholds; the fast comparison applies the same 0.1 % to the stages.

### Verdicts

A scene's snapshots are taken in a fixed order (`tools/rendering/fast/plan.py`): the isolated left eye, the isolated right eye, and multipass are kept as references, each mode takes a second snapshot compared with its first, and single-pass and each variant are compared with every reference. The repeat's counts are the noise floor for that mode, plane, and eye, and a row's floor is the larger of its two modes'. A row passes within the thresholds above, counts as noise when it exceeds them by no more than twice the floor plus 0.02 % of the pixels (`--noise-factor`), and fails otherwise. Against a `--baseline` report, only a row whose count grew by more than that allowance fails. A variant that forces no switch renders the single-pass mode again, which the repeat already does, so it is skipped.

### Dumps and saved planes

With `dump = "on_failure"` the comparisons over the failure limits are dumped, and with `always` every one. The reference's and the test's pixels are read back cropped to the bounding box grown by 16 pixels (the whole eye with `dump_crop = false`, about 50 MB per plane and eye at the headset's size) and written by a writer thread into `captures/<id>.dump/`: the final image as PNG, a stage as its stored bytes compressed with gzip. The comparison's `dump` record names the files, the crop, and the DXGI format and pixel size to decode a raw dump with (`comparison/stages.py`'s `decode_raw`). The acknowledgement is written after its dumps.

With `save = true` (`--save`), every plane is read back whole, eye by eye, into `captures/<id>.dump/<label>.<plane>.eye<N>.png` or `.raw.gz`, whether or not it is compared, and the acknowledgement's `saved` list names each file. The FSR checks read these files.

## FSR variants

FSR's effect is smaller than what drifts between scenes (see "The pinned anchor"), so its settings are compared within one schedule. A config's `[[fsr_variants]]` (`tools/rendering/experiment_fsr.py`) each render, after the configured modes and any single-pass variants, in each of their modes, at the same clock and pinned pose:

```toml
[[fsr_variants]]
name = "fsr-off"                 # the game's own anti-aliasing
fsr = false

[[fsr_variants]]
name = "reference"
fsr = false
antialiasing = false
resolution_scale = 1.414         # twice the pixels; past 1.415 video memory runs out
modes = ["left_only"]
```

A variant can also turn off FSR's jitter, its hybrid velocity decode, or its sharpening, set a sharpening strength, turn off the game's own anti-aliasing with FSR off, or run the mod's TAA in FSR's place with its own tuning ([taa.md](../rendering/taa.md)); `experiment_fsr.py` has the fields and the overrides each maps to. A variant replaces the experiment's FSR overrides with its own. Every configuration resets FSR's history and warms up for the config's `warmup_frames`; FSR settles about 64 frames after a reset at a still camera, so the FSR configs ask for 120. Each variant takes two snapshots, the second compared with the first (at a still camera the next frame, under a motion the same phase a cycle later), and its first is compared with the mode's own first snapshot when that is kept and the variant renders at the session's size. Only the fast comparison runs FSR variants.

A single motion snapshot lands on one jitter offset ([fsr-measurements.md](../rendering/fsr-measurements.md#one-motion-snapshot-is-one-jitter-offset)). `--motion-phase P` takes each first snapshot at phase `P`, and with `--next-frame-repeat` the repeat at `P + 1`, so four runs at successive odd phases cover the eight offsets.

`tools/rendering/suites/fsr.toml` lists the FSR scenes (motion vectors, jitter, and quality) and `suites/taa.toml` the TAA's. `fsr-quality` reads a quality run's saved images and scores each configuration against the reference, frame to frame, and for flicker (run the slow scenes with `--next-frame-repeat`), averaging over several reports when given them; `fsr-jitter` reads a jitter run (`--planes final pre_aa --save`) and compares the shift between each mode's two `pre_aa` planes against the jitter FSR was told. The results are in [fsr-measurements.md](../rendering/fsr-measurements.md).

## A still camera holds a fixed pose

Most coverage configs carry a `[motion]` table with every amplitude zero, which holds the game's camera without knowing the terrain height, and under a motion every capture waits for its phase after a full cycle. The fast comparison holds such a camera with a fixed pose instead: each snapshot is taken on the first frame after the warm-up and its repeat on the next frame. A moving camera runs its motion as configured; `--full-cycles` runs a still one that way too.

### The pinned anchor

The freeze holds whatever camera was live when a scene's first configuration began, and the live camera follows the player's idle body, so two runs of one scene minutes apart started centimetres and degrees apart, which changed the framing and made counts from separate runs incomparable. A configuration request can carry an `[anchor]` table (`position`, and `orientation` as a quaternion), which the payload holds as the full-camera freeze's base in place of the live camera (`pose_control::pin_full_camera`), so a still pose and every phase of a motion are the same to the bit in every run that names it. The fast comparison pins one per scene (`fast/run.py`'s `pinned_anchor`): a fixed-camera scene its own camera, a scene with a `[scene.anchor]` table that, and a staged scene the camera the first fast comparison at its stage froze in this session, recorded in the session's `pinned-anchors.json`. Copying that file into a new session's configuration directory carries the pose across a relaunch. The report records each scene's `anchor`.

The engine places the global illumination's light-propagation volume once per frame from the game camera's render context ([`LightManager::UpdatePlayerLightingTransforms`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gi::LightManager##func-UpdatePlayerLightingTransforms)), which keeps following the live head, so the indirect light changed between snapshots at a frozen camera. A detour on it (`update_player_lighting_transforms` in `crates/jc3vr-view/src/hooks/camera/mod.rs`) hands it the held pose while the harness holds the full camera.

The clouds and their shadows drift between runs (the harness releases the clock between scenes), and the haze can change within a minute; neither moves measurably within a schedule. Compare within a schedule, and treat counts from separate runs as comparable only for the ground under no cloud shadow and only minutes apart. `--repeat-after-seconds` waits before each repeat, so the floor also measures what changes with time at a frozen camera.

Every scene begins with an anchor configuration, as the original path does, though a still camera needs no anchor to aim at: without it, the first configuration after the previous scene's restoration renders differently from those after it, for a reason not measured.

## Reusing the stage

A staged scene's teleport and settle is paid once per location. The harness records the last stage it sent in the session's configuration directory (`last-stage.json`, `tools/rendering/session/stage_record.py`). A stage that matches the record is sent with `reuse_within_m`, and the payload acknowledges it at once, setting the environment again but skipping the teleport, the spawns, and the settle, when the player still stands within that distance of the location; otherwise it stages afresh (`crates/jc3vr-tooling/src/testing/stage.rs`). The acknowledgement says which it did (`reused`). The original path (`python3 -m tools.rendering run`) reuses a stage the same way, and both take `--restage`.

## Re-injection

`iterate` re-injects through the injector's own `--reload`, sent into the game's container the way `scripts/proton_run.sh` sends it (`tools/rendering/session/reinject.py`). The injector asks the payload to eject, waits for its image to leave the process, copies the new build over the loaded name, and injects it; the harness then waits for the new payload's session log to report gameplay. Right after an eject, Wine can keep the ejected image's file open until the injector that ejected it exits, so the copy can fail with a sharing violation: the injector retries it briefly, and the harness then runs a fresh injector, which finds nothing loaded and injects. The next configuration's readiness checks hold the first snapshot until the new payload's session, hooks, and shader patches are all live.

About the eject under the harness:

- It takes about fifteen seconds: the human-hull participant ([human-hull-restoration.md](../body/human-hull-restoration.md)) waits out a bounded restoration that never completes there, then commits with the narrowed hulls in place, a state the re-injected payload inherits.
- A payload that ran the shader identity census (`shader_identity`, which the original path's coverage configs enable) or the texture census (`testing.texture_census`) cannot be ejected, because the census's detours pin the image. `iterate` reads the pin from the session log and asks for a relaunch instead. The fast comparison arms neither.
- The game's video memory grows each time the engine is resized: each round trip into the single-pass double-wide target and back, each resolution-scale round trip, and each eject and re-injection (a resize to the eye size and back). Part of it is the UI reset's background target, which [`UIManager::InitBackGroundTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-InitBackGroundTexture) creates at the new size while Scaleform keeps the old one through [`m_SFBackGroundTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_SFBackGroundTexture) and movies that loaded it; the rest is not identified. The payload's own render-thread resources are released at the eject (`payload/src/eject_gpu.rs`), which logs the video memory before and after. A session that toggles single-pass as often as the standing suite does needs a relaunch every few suites; check `nvidia-smi` when the cycle slows down.

## Pacing and cycle times

Monado's null compositor paces the session's frames, and every harness wait counts frames, so a session takes `--compositor-fps` (120 by default) from the launcher and the game runs as fast as it renders. The payload looks for requests every frame in a harness session and the harness polls for acknowledgements every 5 ms. A scene with its stage reused takes about seven seconds at either eye size, a first visit to a location adds the teleport and settle, and a build, re-injection, and rerun of one scene takes under a minute, most of it the build and the eject. The standing suite takes about five minutes, mostly in its stages and its moving cameras, whose every snapshot waits out a motion cycle. `--settle-seconds` shortens the settles at the risk of comparing a world still streaming in, and `--only` runs a subset.
