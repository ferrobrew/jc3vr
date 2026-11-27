# Fast comparison

The capture harness (`tools/rendering/`, with the payload side in `crates/jc3vr-tooling/src/testing/` and `crates/jc3vr-capture/src/harness/`) compares single-pass stereo against the isolated eyes and multipass on a frozen scene. Its original path renders each mode several times, reads every capture and stage back to files, and compares them in Python; it stays for anything that needs the full stage set or the diagnostics. The fast comparison renders each mode once plus a repeat, keeps the images on the GPU, compares them there, and reads back only a summary. It is the inner loop for single-pass work: build, re-inject, and rerun a list of scenes against a live session in under a minute, with no relaunch and no world load.

The render hooks reach the harness only through the probe table the capture crate registers (the render crate's `probes`, filled by `harness/register_probes.rs`), calling it at generic events, so a new experiment widens the harness rather than the hooks. Until a request arms them, the hooks skip the table after one relaxed load (`probes/arming.rs`).

## The fast loop

Launch a session once (`--build-only` requires only the build check, for a session that only serves the inner loop), then iterate after each change:

```sh
python3 -m tools.rendering.session.matched_run N --gpu <vendor:device> --prior M --unmatched --build-only
python3 -m tools.rendering iterate --session N --only town tunnel
python3 -m tools.rendering iterate --session N                      # the whole standing suite
```

`iterate` builds incrementally, re-injects, and runs the scenes; `fast` runs them against whatever payload is loaded. Both print each plane's differing pixel count over its noise floor, write `fast-report.json` under `target/harness-probe/fast/<stamp>/`, and exit 0 when nothing fails, 1 when something does, and 2 when a scene could not be compared (`tools/rendering/fast/cli.py`). Run the full checks once before committing.

Every harness build enables the payload's `capture-rewrites` feature, which compiles in the shader rewrites only the committed experiments request; a payload built without it refuses those requests with an error naming the feature.

`tools/rendering/suites/standing.toml` lists the regression matrix of scenes, each naming a config under `tools/rendering/experiments/`.

## Snapshots

A `snapshot` request (`crates/jc3vr-capture/src/harness/snapshot/`, `tools/rendering/protocol/snapshot.py`) keeps the next eligible frame's planes as GPU textures under a label, chosen as a screenshot's frame is. The planes are the final image, the G-buffer's, and points through the post chain, FSR's inputs, and the ambient occlusion (the names are in `protocol/snapshot.py`). Each is one GPU copy, so a snapshot never stalls.

Comparison runs in `snapshot_diff_cs` (`crates/jc3vr-capture/shaders/`), each thread group reducing a heat-map cell to counts, differences, and a bounding box that are read back in one copy (`summary.rs`). With `dump`, failing comparisons' pixels are read back cropped to their bounding box into `captures/<id>.dump/`; with `save`, every plane is written whole.

The differing rules match the Python comparison's (`comparison/images.py` and `comparison/stages.py`), so the counts are comparable: the final image counts a pixel over a channel threshold, and the stages count any changed bit, except that depth may allow a tolerance in representable floats (`--depth-ulps`), because single-pass's reprojected transforms and an isolated eye's matrices round differently and land a surface a few floats apart.

### Verdicts

The isolated eyes and multipass are kept as references, each mode takes a repeat compared with its first snapshot, and single-pass and each variant are compared with every reference (`tools/rendering/fast/plan.py`). The repeat's counts are that mode's noise floor: a row over the thresholds but within a multiple of the floor (`--noise-factor`) counts as noise, and beyond it fails. Against a `--baseline` report, only a row that grew past that allowance fails.

### FSR's jitter

Two snapshots on different FSR jitter offsets ([fsr.md](../rendering/fsr.md#sub-pixel-jitter)) differ at every edge, so a still camera's snapshot names the offset it wants (`jitter_phase`) and waits for it, and its repeat takes the next offset. A moving camera's motion period is a whole number of sequences, so a phase falls on the same offset in every configuration.

## FSR variants

FSR's effect is smaller than what drifts between runs, so its settings are compared within one schedule: a config's `[[fsr_variants]]` (`tools/rendering/experiment_fsr.py`, which has the fields) each render after the configured modes at the same clock and pinned pose, turning FSR, its jitter, decode, or sharpening, the game's anti-aliasing, or the mod's TAA ([taa.md](../rendering/taa.md)) on or off, or changing the resolution scale. Every configuration resets FSR's history and warms up long enough for FSR to settle. `tools/rendering/suites/fsr.toml` and `suites/taa.toml` list the scenes, and the `fsr-quality`, `fsr-jitter`, and `fsr-motion` commands read the saved planes; the results are in [fsr-measurements.md](../rendering/fsr-measurements.md).

## A still camera holds a fixed pose

A still camera's snapshot is taken on the first frame after the warm-up and its repeat on the next, instead of waiting out a zero-amplitude motion cycle as the original path does. The live camera follows the player's idle body, so separate runs would start centimetres apart. A configuration request can carry an `[anchor]` pose, which the payload holds as the freeze's base (`pose_control::pin_full_camera`), so every run that names it renders the same view to the bit. The fast comparison pins one per scene (`fast/run.py`'s `pinned_anchor`): a fixed camera's own, a `[scene.anchor]`, or for a staged scene the camera its first visit froze, recorded in the session's `pinned-anchors.json`, which can be copied into a new session.

The engine places the global illumination's light-propagation volume each frame from the game camera ([`LightManager::UpdatePlayerLightingTransforms`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gi::LightManager##func-UpdatePlayerLightingTransforms)), which follows the live head, so a detour (`update_player_lighting_transforms` in `crates/jc3vr-view/src/hooks/camera/mod.rs`) hands it the held pose while the harness holds the camera.

The clouds and haze drift between runs, so compare within a schedule. Every scene begins with an anchor configuration, because without one its first configuration renders differently from those after it.

## Reusing the stage

A staged scene's teleport and settle is paid once per location. The harness records the last stage it sent (`last-stage.json`, `tools/rendering/session/stage_record.py`), and a matching stage is sent with `reuse_within_m`: the payload sets the environment again but skips the teleport, spawns, and settle when the player still stands within that distance (`crates/jc3vr-tooling/src/testing/stage.rs`). The original path reuses stages the same way, and both take `--restage`.

## Re-injection

`iterate` re-injects through the injector's `--reload` (`tools/rendering/session/reinject.py`). Wine can keep the ejected image open until the ejecting injector exits, so on a sharing violation the harness runs a fresh injector.

A payload that ran the shader identity census or the texture census cannot be ejected, because the census's detours pin the image, so `iterate` asks for a relaunch; the fast comparison arms neither. The game's video memory grows with each engine resize, each single-pass toggle, and each re-injection, partly through the UI's background texture, which Scaleform keeps at the old size ([`UIManager::InitBackGroundTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-InitBackGroundTexture)), so a session needs a relaunch every few suites.

Monado's null compositor paces the session (`--compositor-fps`), and every harness wait counts frames.
