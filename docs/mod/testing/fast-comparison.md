# Fast comparison

The capture harness (`tools/rendering/`, with the payload side in `crates/jc3vr-tooling/src/testing/` and `crates/jc3vr-render/src/screenshot/harness/`) compares single-pass stereo against the isolated eyes and multipass on a frozen scene. The render hooks reach the payload side only through the probe tables it registers at startup, one for the graphics-engine pipeline's seams (`hooks/graphics_engine/probes.rs`, registered by `screenshot/harness/register_pipeline.rs`) and one for the single-pass draw path (`stereo/single_pass/probes.rs`); a probe nobody registered is inert. Its original path renders each mode three times, reads every capture and its stages back to files, and compares them in Python afterwards. The fast comparison renders each mode once (plus one repeat frame), keeps the images on the GPU, compares them there, and reads back only a summary. It is the inner loop for single-pass work: build, re-inject, and rerun a list of scenes in about a minute against a live session, with no relaunch, no world load, and no checks beyond the build. The original path stays for anything that needs the full stage set or the diagnostics, and its reports read the same captures as before.

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

Options: `--planes` picks the planes (the final image is always compared), `--velocity` adds the velocity buffer, `--dump never|on_failure|always` and `--no-crop` control the dumps, `--baseline <fast-report.json>` judges regressions against an earlier run, `--restage` stages afresh, `--full-cycles` runs a motion that never moves the camera as configured, `--no-repeat` skips the noise-floor repeats, `--save` writes every snapshot's planes whole (see "Saving planes"), `--next-frame-repeat` takes a moving camera's repeat on the next frame of the motion rather than a cycle later, `--jitter-phase` picks the FSR jitter offset a still camera's snapshots wait for (see "FSR's jitter in a snapshot"), and `--warmup-frames` and `--settle-seconds` override the configs' warm-up and settle.

## The standing suite

`tools/rendering/suites/standing.toml` lists the regression matrix: the town at midday, the rotating and translating cameras there, the shipped settings static and rotating, the town under each headset rig (`index`, `quest`, `stress`, and `beyond`), the tunnel, the meadow, the capital at night, the shore lake, and the cliff at dusk. A suite entry names a config under `tools/rendering/experiments/` and may add `headset = "<rig>"` (the `[visual_overrides] headset` of every mode) or a `[scene.stage]` table for a fixed-camera config that carries none; the two production views are staged below their cameras, as the capture batches do. Entries at one location with one environment are listed together, so the stage runs once for them. The suite runs at the session's eye size: a session launched with `--resolution-scale 1.0 --eye-size 3584x3561` runs it at the headset's.

## Snapshots

A `snapshot` request (`crates/jc3vr-render/src/screenshot/harness/snapshot/`, `tools/rendering/protocol/snapshot.py`) keeps the next eligible frame's planes as GPU textures under a label:

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

Beyond the geometry planes, a snapshot can keep the scene colour at five points of the frame, each an R11G11B10 float plane: `post_resolve` (once the deferred lights' resolve has lit it), `pre_atmosphere` and `post_atmosphere` (around the atmospheric scattering), `post_sun` (after the sun pass), and `main_color` (at the start of the post chain, before the tone map), and the tone-mapped image the anti-aliasing reads and writes (`pre_aa`, `post_aa`). Their counts are of any changed bit, which a float plane changes at a large share of its pixels between any two renders; read them from dumps, with a tolerance in representable values.

FSR's inputs have two planes of their own, taken right after the velocity decode for each eye it resolves: `fsr_motion`, the decoded motion vectors FSR is handed (two 16-bit floats per pixel, in UV before FSR's motion-vector scale), and `fsr_depth`, the depth handed with them (the engine's depth for an eye rendered whole, the decode's eye-sized copy for a half of the single-pass target). Each is one eye's texture in every mode, so a single-pass snapshot keeps them per eye rather than splitting a double-wide plane. They exist only where FSR runs, and the fast runner leaves them out of a configuration whose overrides do not put FSR back. Every snapshot's acknowledgement also carries `fsr`, one record per eye FSR resolved on the snapshot's frame (`fsr::DispatchRecord`): the decode's switches and sign convention, FSR's motion-vector scale, the eye's view-projections this frame and last, the clip-to-previous-clip matrices the decode reprojected with, and the previous view-projection the engine's velocity pass encoded against. `python3 -m tools.rendering fsr-motion` checks the saved planes against those records; see "Verifying the motion vectors" in [fsr.md](../rendering/fsr.md).

For the ambient occlusion, `ssao` is the scene target the SSAO composite multiplies the occlusion into (its alpha), as the block leaves it, `ssao_volumes` the occlusion volumes' target the composite reads, and `ssao_step0` to `ssao_step7` the render target each draw inside the SSAO block leaves, in order, kept from a hook after the engine's draw call. A step plane may be smaller than the eye (the block's half-resolution targets); it is compared at its own size, the whole texture for an isolated eye and each half for single-pass, and its counts are against its own pixel count.

The frame is chosen as a screenshot's is: in gameplay, with the VR session rendering and the mode's eyes copied fresh, once the visual configuration is ready, and under a motion configuration at the requested phase after a full cycle. The final image is copied from the same source a screenshot reads (the collapsed back buffer, the isolated eye's, or both eye captures in multipass), and the geometry planes from the stage hooks at the end of the G-buffer range, where the screenshot's stages are read: the depth buffer, `gbuffer0` (albedo), `gbuffer1` (normals), and the velocity buffer. Each plane is copied with one GPU copy into a texture of its format family, so taking a snapshot never stalls. A single-pass plane holds both eyes side by side and is split at the eye width.

The snapshot is then compared with each kept snapshot `compare` names, plane by plane and eye by eye where both have the eye, by `snapshot_diff_cs` (`crates/jc3vr-render/shaders/snapshot_diff_cs.hlsl`): one thread group per cell of a 32 by 32 heat map over the eye, each reducing its cell to the differing pixel count, the sum of the per-pixel mean channel difference, the largest channel difference, the differing pixels' bounding box, and the count of pixels whose stored values change at all. The cells of every comparison are read back in one copy (about 32 KB a comparison) and totalled on the CPU (`summary.rs`). The acknowledgement, `captures/<id>.json`, lists each comparison's reference, plane, eye, pixel count, differing count, changed count, mean difference, largest difference, bounding box, and heat map, with how many frames the snapshot waited and how long the copies, comparisons, and readback took on the render thread. A snapshot that is not kept is released once compared, and `snapshot_release` releases every kept one; the fast runner sends it at the end of each scene, and an eject releases them too. The run 296 town at 3584 by 3561 per eye took 3 to 7 ms of render-thread time per snapshot, its comparisons included.

### FSR's jitter in a snapshot

While FSR runs, every frame's projections carry the next offset of its eight-phase jitter sequence ("Sub-pixel jitter" in [fsr.md](../rendering/fsr.md)), and two configurations whose snapshots land on different offsets differ at every edge. The sequence restarts with every configuration's FSR history reset, but the frames between the reset and the snapshot vary with the mode's readiness checks, so a snapshot names the offset it wants instead: with `jitter_phase` set and the camera still, it waits for a frame at that index of the sequence (at most seven frames). The fast comparison asks a still camera's first snapshot for index 0 and its repeat for index 1, the next frame, or `--jitter-phase K` and `K + 1`; a configuration without FSR (the capture profile turns it off) does not send the field, and with `fsr.jitter` off the payload ignores it. A moving camera's snapshot is taken at its motion phase; the sequence restarts when the motion begins, so with a period that is a whole number of sequences (the configs' 120 frames) a phase falls on the same offset in every configuration, and the snapshot's FSR records report it (`jitter_phase`, `jitter_pixels`). A payload built before the field rejects such a request as unparseable (`unknown field jitter_phase` in its session log) and never acknowledges it, so the snapshot times out: that was the `shipped-static` timeout in run 332, whose loaded payload predated the field, and the scene takes its snapshots once the current payload is injected. Since the first snapshot of a still camera asks for index 0 and its repeat for index 1, a still FSR scene's repeat lands on another offset, and its noise floor is the difference between two offsets (about 600,000 pixels per eye at the town), not the frame-to-frame noise. In run 327 at the town, single-pass against multipass with FSR at the same offset differed at 82,000 and 112,000 pixels per eye, where at different offsets they had differed at 1.35 and 1.05 million.

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

### Saving planes

With `save = true` (`--save`), every plane of the snapshot is read back whole, eye by eye, and written to `captures/<id>.dump/<label>.<plane>.eye<N>.png` (the final image) or `.raw.gz` (any other plane, as stored), whether or not it is compared; the acknowledgement's `saved` list names each file with its size and DXGI format. It is for analysis offline: the FSR checks read these files. A saved final image at the headset's size is about 20 MB, a depth plane 40 to 60 MB.

## FSR variants

FSR's effect is smaller than what drifts between scenes (the clouds and the haze, see "The pinned anchor"), so its settings are compared within one schedule. A config's `[[fsr_variants]]` (`tools/rendering/experiment_fsr.py`) each render, after the configured modes and any single-pass variants, in each of their modes (`modes`, by default every configured one), at the same clock and pinned pose:

```toml
[[fsr_variants]]
name = "fsr-off"                 # the game's own anti-aliasing, as the capture profile leaves it
fsr = false

[[fsr_variants]]
name = "hybrid-off"
hybrid_velocity = false

[[fsr_variants]]
name = "no-aa"                   # no anti-aliasing at all
fsr = false
antialiasing = false

[[fsr_variants]]
name = "reference"
fsr = false
antialiasing = false
resolution_scale = 1.414         # twice the pixels; at most 1.415, past which video memory runs out
modes = ["left_only"]
```

A variant can also turn off FSR's sub-pixel jitter (`jitter = false`, the `fsr_jitter = "disabled"` override), its sharpening (`sharpen = false`, `fsr_sharpen`), or turn the sharpening on at a strength (`sharpness = 0.5`, `fsr_sharpness`), and with FSR off it can turn the game's own anti-aliasing off too (`antialiasing = false`, the `anti_aliasing = "disabled"` override, which drops the engine's anti-aliasing to none: `post_fx.skip_anti_aliasing`), for an image with none. A variant replaces the experiment's FSR overrides with its own, and `resolution_scale` sets the `[visual_overrides] resolution_scale` override (the render's `vr.resolution_scale`, each side, within [0.5, 1.415]; the payload puts the session's back at the next configuration). Every configuration resets FSR's history and warms up for the config's `warmup_frames`, so the variants accumulate alike; FSR settles about 64 frames after a reset at a still camera, so the FSR configs ask for 120. Each variant takes two snapshots, the second compared with the first: at a still camera the next frame (the frame-to-frame change), and under a motion the same phase a cycle later. Its first is compared with the mode's own first snapshot when that is kept and the variant renders at the session's size. A variant's snapshots are labelled `f<N>_<name>_<mode>_a` and `_b`, and its repeat is its own noise floor. Only the fast comparison runs FSR variants; the original path refuses a config that has them.

A variant can also run the mod's own temporal resolve in FSR's place (`taa = true`, the `taa = "enabled"` override; the jitter and the hybrid decode follow `jitter` and `hybrid_velocity`, and the sharpening settings are refused with it), and tune it: `history_weight`, `motion_history_weight`, `clip_gamma_rest`, and `current_sharpness` in tenths, the `taa_history_weight`, `taa_motion_history_weight`, `taa_clip_gamma_rest`, and `taa_current_sharpness` overrides (see [taa.md](../rendering/taa.md)). A configuration that runs either resolve waits for the jitter offset at a still camera as FSR's does. `tools/rendering/suites/taa.toml` lists its scenes.

A single motion snapshot lands on one of the jitter's eight offsets ("One motion snapshot is one jitter offset" in [fsr.md](../rendering/fsr.md)). `--motion-phase P` takes each first snapshot at phase `P` in place of the config's, and with `--next-frame-repeat` the repeat at `P + 1`, so four runs at phases 15, 17, 19, and 21 cover the eight offsets. `fsr-quality` scores each repeat against the reference's repeat, and given several reports it averages each scene's scores over all their snapshots.

`tools/rendering/suites/fsr.toml` lists the FSR scenes: the motion-vector scenes at the town (still, rotating, translating, rotating under the `index` rig, and rotating with every shipped setting), the jitter scenes (`jitter-shift`, and under the `index` rig), and the quality scenes at the town and the meadow (still, rotating, translating, and panning slowly, with FSR without its jitter, the game's SMAA, no anti-aliasing, and a reference at twice the pixels with no anti-aliasing). `python3 -m tools.rendering fsr-quality` reads the saved images of a quality run: against the reference, the frame-to-frame change, and the flicker, the change of each configuration's error against the reference from the first snapshot to the repeat, which takes out what the camera's own motion changes (run the slow scenes with `--next-frame-repeat`, so the repeat is the next frame). `python3 -m tools.rendering fsr-jitter` reads a jitter run (`--planes final pre_aa --save`): the shift between each mode's two `pre_aa` planes against the change in the jitter FSR was told, and what the shift does not explain, at full resolution and averaged over blocks. The results are in [fsr.md](../rendering/fsr.md).

## A still camera holds a fixed pose

Most coverage configs carry a `[motion]` table with every amplitude zero, which holds the game's own camera without knowing the location's terrain height. Under the motion, every capture waits for its phase after a full cycle, 120 frames at the configs' period. The camera never moves, so the fast comparison holds the same camera with a fixed pose instead (the anchor with no offset): each snapshot is taken on the first frame after the warm-up, and its repeat on the next frame. A moving camera runs its motion as configured, cycles and all; `--full-cycles` runs a still one that way too.

### The pinned anchor

The freeze holds whatever camera was live when a scene's first configuration began, and the live camera follows the player's idle body and the game's camera state, so two runs of the town minutes apart started 10 to 20 cm and several degrees of yaw apart (run 312's anchors). The framing moved with it (a telegraph pole in front of the ground in one run and not another), and with it how much of every surface was in view, which made counts from separate runs incomparable. Every configuration request can now carry an `[anchor]` table (`position`, `orientation` as a quaternion), which the payload holds as the full-camera freeze's base in place of the live camera (`pose_control::pin_full_camera`), so a still pose and every phase of a motion are the same to the bit in every run that names it. The fast comparison pins one per scene (`fast::run::pinned_anchor`): a fixed-camera scene its own camera, a scene with a `[scene.anchor]` table that, and a staged scene the camera the first fast comparison at its stage froze in this session, recorded in the session's `pinned-anchors.json` and reused by every later scene at that stage. Copying that file into a new session's configuration directory carries the pose across a relaunch. The report records each scene's `anchor`. With it, five runs of the town gave single-pass against the isolated left eye 74,775 to 86,778 pixels and the depth 716 to 730, where unpinned runs had spread from 50,000 to a million.

The freeze pins the render camera and the active camera, but the engine places the global illumination's light-propagation volume once per frame from the game camera's render context ([`LightManager::UpdatePlayerLightingTransforms`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gi::LightManager##func-UpdatePlayerLightingTransforms)), which keeps following the live head, so the indirect light on every lit pixel changed between snapshots at a frozen camera. A detour on it (`update_player_lighting_transforms` in `hooks/camera/mod.rs`) hands it the held pose while the harness holds the full camera. Before it, multipass differed from the isolated eyes at 17,000 to 50,000 pixels at the town and an isolated eye differed from itself four to six seconds later at 27,000 to 1.4 million; after it, multipass matches the isolated eyes within the repeat (3,400 to 7,600 against a floor of 2,500 to 6,200), and the four-second repeat stays at 1,500 to 6,000.

Two things still change over minutes and are not held: the clouds in the sky and their shadows on the ground drift between runs (the harness releases the clock between scenes), and the haze changed between two runs a minute apart. Holding the weather's rate ([`Weather`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/environment::Weather)'s acceleration) held the clouds but not the haze, and was taken out again. Within a schedule neither moves measurably, so compare within a schedule, and treat counts from separate runs as comparable only for the ground under no cloud shadow and only minutes apart at most.

`--repeat-after-seconds` waits before each repeat snapshot, so the floor also measures what changes with time at a frozen camera.

Every scene still begins with an anchor configuration, as the original path does, though a still camera needs no anchor to aim at. Without it, the first configuration after the previous scene's restoration rendered differently from those after it: in run 297 both of the isolated left eye's snapshots from it differed from multipass at 114,906 of 115,200 pixels while agreeing with each other, and with the anchor first they differed at 125. Why is unmeasured.

## Reusing the stage

A staged scene's teleport and settle (20 seconds in the configs, on top of the streaming) is paid once per location. The harness records the last stage it sent in the session's configuration directory (`last-stage.json`, `tools/rendering/session/stage_record.py`): the location, the spawns, and the environment. A stage that matches the record is sent with `reuse_within_m = 5.0`, and the payload acknowledges it at once, with the environment set again but no teleport, spawns, or settle, when the player still stands within that distance of the location; otherwise it stages afresh (`crates/jc3vr-tooling/src/testing/stage.rs`). The acknowledgement says which it did (`reused`). The original path (`python3 -m tools.rendering run`) reuses a stage the same way, and both take `--restage` to stage afresh.

## Re-injection

`iterate` re-injects through the injector's own `--reload` (`injector/src/main.rs`), sent into the game's container the way `scripts/proton_run.sh` sends it, over the session's private bus (`tools/rendering/session/reinject.py`). The injector asks the payload to eject, waits for its image to leave the process, copies the new build to the loaded name, injects it, and runs it; the harness then waits for the new payload's session log to report gameplay. Right after an ejection, the copy of the new build over the loaded name can fail with a sharing violation for tens of seconds (in run 296 it outlasted a 30-second retry in one injector and succeeded at once in the next), which looks like Wine keeping the ejected image's file open until the injector that ejected it exits. The injector retries the copy for two seconds, and the harness then runs a fresh injector, which finds nothing loaded and injects. The new payload starts its OpenXR session, installs its hooks, and patches the shaders on its own; the next configuration's readiness checks (the shaders re-created under the requested state, the XR frame identity, and the rest) hold its first snapshot until they all pass.

In run 295, three runs of the town at 320 by 360 per eye on the freshly launched payload and three after a re-injection gave the same ranges: single-pass against the isolated left eye at 661 to 1,528 final-image pixels before and 897 to 2,089 after, against the right eye 860 to 2,422 and 1,654 to 2,009, and multipass against the isolated eyes 101 to 634 and 125 to 558. The geometry planes were bimodal in both, an eye's albedo differing at about 30 pixels or at 2,000 to 9,000, as the terrain patches' detail state is documented to do from configuration to configuration ("Capturing varied scenes" in [single-pass-stereo.md](../stereo/single-pass-stereo.md)).

Two things about the eject itself:

- The game's video memory grew by about 3 to 4 GB with each eject and re-injection in run 310 (10 GB after the launch, 32 GB after the fifth re-injection and a few runs with dumps), and at the card's 32 GB the frame time rose to about 540 ms. Two causes were found in runs 311 and 312. The first was the payload's own: render paths that build on the render thread keep their resources in statics, which an unloading DLL never drops, and the eject renders no frames after its cleanups, so the per-frame paths that would have released them never ran. The eject now releases them explicitly once the engine is back at its native size (`payload/src/eject_gpu.rs`): FSR's contexts and textures, the redirected HUD's colour and depth target (248 MB at the headset's size), the controller overlay's and the scope lens's pipelines (which held render-target views over the engine's back buffer), the world markers, the HUD atlas, the mirror overlay, and the floating panel. The payload logs the game's video memory when the eject begins and once the hooks are removed (`video memory` lines), and each holder's release at debug level. The second is not the payload's resources: the game's video memory grows each time the engine is resized, by about 560 MB per round trip into the single-pass double-wide target and back at 3584 by 3561 per eye (three successive town scenes in one payload, with the stage reused: 11.2, 11.8, 12.3, and 12.9 GB), by about 300 to 600 MB per resolution-scale round trip in multipass, and by 236 MB per idle eject and re-injection (the resize to the eye size and back), exactly, cycle after cycle. Scenes that use only the isolated eyes, which never resize, add nothing (two in a row left it at 12,887 MB). It is unchanged with the HUD redirect or the mod-owned back buffer off. Run 318 found part of it with a census of the live 2D textures (`testing.texture_census`, `debug::texture_census`: it detours the device's texture creation and the textures' release, and logs the live textures grouped by size, format, and creating call stack after every resize; once armed the payload cannot be ejected, and the re-injection refuses it). Each round trip leaves one display-sized RGBA8 render target of each size alive, created by [`UIManager::InitBackGroundTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-InitBackGroundTexture) in the UI reset: about 147 MB at the headset's size, every other texture group balancing out. The engine destroys its own handle to the previous background, but the target is wrapped in a Scaleform texture and image ([`m_SFBackGroundTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_SFBackGroundTexture)) that movies loading the background keep. Keeping one image for good and moving each new texture into it (run 319) did not release the old targets, so something past the image (Scaleform's deferred texture release or its render caches) still holds them; that attempt is not in the tree. The census accounts for 150 to 180 MB of the 560 MB a round trip adds as the adapter counts it; the rest is not in 2D textures created after the census was armed (buffers, or memory DXVK keeps), and is not identified. A session that has toggled single-pass many times, as the standing suite does about a dozen times, still needs a relaunch every few suites; check `nvidia-smi` when the cycle slows down.
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
