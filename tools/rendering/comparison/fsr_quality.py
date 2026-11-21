"""Measure FSR's image against the game's own anti-aliasing and a supersampled reference.

Reads a fast report of the FSR quality scenes (`tools/rendering/suites/fsr.toml`, run with `--save`),
whose every configuration rendered in one schedule at one clock and pose: FSR as shipped (the mode's
own snapshots), FSR without its sub-pixel jitter (`jitter-off`), the game's own anti-aliasing
(`fsr-off`), no anti-aliasing at all (`no-aa`), and the reference (`reference`: the left eye alone at
twice the pixels with no anti-aliasing, downsampled here by area to the eye's size, so the reference
favours no anti-aliasing method). A configuration at a reduced size is scaled back up bilinearly, as a
compositor would.

Per scene it reports, on 8-bit RGB (luma for the gradients):

- against the reference (the left eye): the mean absolute difference, PSNR, the edge sharpness
  (the mean luma gradient where the reference's gradient is in its top tenth, over the reference's
  own, so 1 is as sharp as the reference), and the ghosting (the mean gradient the image has beyond
  the reference's where the reference is flat, below its median gradient: edges that are not there);
- against the game's anti-aliasing in the same mode: the pixels more than three codes apart and the
  mean difference;
- each configuration's repeat: the next frame at a still camera (the frame-to-frame change, which is
  shimmer), or under a motion the same phase a cycle later, or the next frame with
  `fast --next-frame-repeat`;
- the flicker, for the left eye: the mean change of the image's error against the reference from the
  first snapshot to the repeat, `|(b - a) - (reference_b - reference_a)|`, which takes out the change
  the camera's own motion makes and leaves the frame-to-frame instability (shimmer, crawling edges);
  with the next-frame repeat it measures shimmer in motion;
- between modes: multipass against single-pass, eye by eye, per configuration.

A repeat the reference also took is scored against the reference's repeat. Given several reports
(one per pair of motion phases, `fast --motion-phase P --next-frame-repeat`), it also averages each
scene's scores against the reference, and its flicker, over every snapshot of every report: in
motion, the average over the jitter's offsets.

    python3 -m tools.rendering fsr-quality --session N target/harness-probe/fast/<stamp>/fast-report.json
    python3 -m tools.rendering fsr-quality --session N <phase-15>/fast-report.json <phase-17>/fast-report.json ...
"""

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

SESSIONS = Path("target/harness-probe")
BASE = "fsr"
THRESHOLD = 3


class QualityError(ValueError):
    pass


def load(captures: Path, record: dict, size: tuple[int, int] | None = None) -> np.ndarray:
    """A saved final image as float32 RGB, resampled to `size` (width, height) when given: by area
    when it is larger, bilinearly when it is smaller."""
    if record.get("error"):
        raise QualityError(f"fsr-quality: {record['stem']} was not saved: {record['error']}")
    image = Image.open(captures / record["file"]).convert("RGB")
    if size is not None and image.size != size:
        larger = image.size[0] > size[0]
        image = image.resize(size, Image.Resampling.BOX if larger else Image.Resampling.BILINEAR)
    return np.asarray(image, dtype=np.float32)


def luma(image: np.ndarray) -> np.ndarray:
    return image @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)


def gradient(channel: np.ndarray) -> np.ndarray:
    """The Sobel gradient magnitude, zero at the border."""
    padded = np.pad(channel, 1, mode="edge")
    gx = ((padded[:-2, 2:] + 2 * padded[1:-1, 2:] + padded[2:, 2:])
          - (padded[:-2, :-2] + 2 * padded[1:-1, :-2] + padded[2:, :-2]))
    gy = ((padded[2:, :-2] + 2 * padded[2:, 1:-1] + padded[2:, 2:])
          - (padded[:-2, :-2] + 2 * padded[:-2, 1:-1] + padded[:-2, 2:]))
    return np.hypot(gx, gy) / 8.0


def difference(a: np.ndarray, b: np.ndarray) -> dict:
    delta = np.abs(a - b)
    return {"differing": int(np.count_nonzero(delta.max(axis=2) > THRESHOLD)),
            "mean": round(float(delta.mean()), 4)}


def against_reference(image: np.ndarray, reference: np.ndarray, reference_gradient: np.ndarray,
                      edges: np.ndarray, flat: np.ndarray) -> dict:
    delta = image - reference
    mse = float(np.mean(delta ** 2))
    test_gradient = gradient(luma(image))
    return {
        "mean": round(float(np.abs(delta).mean()), 4),
        "psnr": round(10 * np.log10(255.0 ** 2 / mse), 3) if mse > 0 else float("inf"),
        "sharpness": round(float(test_gradient[edges].mean() / reference_gradient[edges].mean()), 4),
        "ghosting": round(float(np.maximum(test_gradient - reference_gradient, 0)[flat].mean()), 4),
    }


def scene_images(scene: dict) -> dict[tuple[str, str, int, str], dict]:
    """The scene's saved final images by (configuration, mode, eye, snapshot letter)."""
    images = {}
    for snapshot in scene.get("snapshots", []):
        step = snapshot.get("step", "")
        config, _, mode = step.rpartition(" ")
        config = config or BASE
        letter = snapshot["label"].rsplit("_", 1)[-1]
        for item in snapshot.get("saved", []):
            label, plane, eye = item["stem"].rsplit(".", 2)
            if plane == "final":
                images[(config, mode, int(eye.removeprefix("eye")), letter)] = item
    return images


def measure_scene(scene: dict, captures: Path) -> dict:
    images = scene_images(scene)
    result = {"scene": scene["name"], "reference": [], "against_fsr_off": [], "repeat": [],
              "flicker": [], "between_modes": []}
    reference_key = ("reference", "left_only", 0, "a")
    eye_key = next((key for key in images if key[0] == BASE and key[2] == 0), None)
    if reference_key in images and eye_key is not None:
        size = (images[eye_key]["width"], images[eye_key]["height"])
        reference = load(captures, images[reference_key], size)
        # Each snapshot against the reference's snapshot of the same letter: the repeat, when the
        # reference has one, is scored against the reference's repeat, so a next-frame repeat in
        # motion scores a second jitter offset.
        for snapshot_letter in ("a", "b"):
            key = ("reference", "left_only", 0, snapshot_letter)
            if key not in images:
                continue
            letter_reference = (reference if snapshot_letter == "a"
                                else load(captures, images[key], size))
            reference_gradient = gradient(luma(letter_reference))
            edges = reference_gradient >= np.percentile(reference_gradient, 90)
            flat = reference_gradient <= np.percentile(reference_gradient, 50)
            for (config, mode, eye, letter), record in sorted(images.items()):
                if eye != 0 or letter != snapshot_letter or config == "reference":
                    continue
                image = load(captures, record, size)
                result["reference"].append({"config": config, "mode": mode,
                                            "snapshot": snapshot_letter,
                                            **against_reference(image, letter_reference,
                                                                reference_gradient, edges, flat)})
        reference_repeat = images.get(("reference", "left_only", 0, "b"))
        if reference_repeat is not None:
            reference_change = load(captures, reference_repeat, size) - reference
            for (config, mode, eye, letter), record in sorted(images.items()):
                repeat = images.get((config, mode, eye, "b"))
                if eye != 0 or letter != "a" or repeat is None:
                    continue
                change = load(captures, repeat, size) - load(captures, record, size)
                flicker = np.abs(change - reference_change)
                result["flicker"].append({
                    "config": config, "mode": mode,
                    "mean": round(float(flicker.mean()), 4),
                    "differing": int(np.count_nonzero(flicker.max(axis=2) > THRESHOLD))})
    for (config, mode, eye, letter), record in sorted(images.items()):
        if letter != "a" or config in ("fsr-off", "reference"):
            continue
        off = images.get(("fsr-off", mode, eye, "a"))
        if off is not None and (off["width"], off["height"]) == (record["width"], record["height"]):
            result["against_fsr_off"].append({"config": config, "mode": mode, "eye": eye,
                                              **difference(load(captures, record),
                                                           load(captures, off))})
    for (config, mode, eye, letter), record in sorted(images.items()):
        repeat = images.get((config, mode, eye, "b"))
        if letter == "a" and repeat is not None:
            result["repeat"].append({"config": config, "mode": mode, "eye": eye,
                                     **difference(load(captures, record), load(captures, repeat))})
    for (config, mode, eye, letter), record in sorted(images.items()):
        other = images.get((config, "single_pass", eye, "a"))
        if mode == "multipass" and letter == "a" and other is not None:
            result["between_modes"].append({"config": config, "eye": eye,
                                            **difference(load(captures, record),
                                                         load(captures, other))})
    return result


def describe(result: dict) -> str:
    lines = [f"{result['scene']}:"]
    for row in result["reference"]:
        lines.append(f"  vs reference  {row['config']:10} {row['mode']:11} "
                     f"{row.get('snapshot', 'a')}  mean {row['mean']:.3f}  "
                     f"PSNR {row['psnr']:.2f} dB  sharpness {row['sharpness']:.3f}  "
                     f"ghosting {row['ghosting']:.3f}")
    for name in ("against_fsr_off", "repeat"):
        for row in result[name]:
            lines.append(f"  {name:14}{row['config']:10} {row['mode']:11} eye {row['eye']}  "
                         f"{row['differing']:>9} px  mean {row['mean']:.3f}")
    for row in result["flicker"]:
        lines.append(f"  flicker       {row['config']:10} {row['mode']:11} eye 0  "
                     f"{row['differing']:>9} px  mean {row['mean']:.3f}")
    for row in result["between_modes"]:
        lines.append(f"  multipass vs single-pass  {row['config']:10} eye {row['eye']}  "
                     f"{row['differing']:>9} px  mean {row['mean']:.3f}")
    return "\n".join(lines)


def average(results: list[dict]) -> list[dict]:
    """Each scene's scores against the reference, and its flicker, averaged over every snapshot of
    every report given: with one report per pair of motion phases (`fast --motion-phase P
    --next-frame-repeat`), the average over the jitter's offsets. PSNR is averaged in decibels."""
    groups: dict[tuple[str, str, str], dict[str, list[float]]] = {}
    for result in results:
        for row in result["reference"]:
            group = groups.setdefault((result["scene"], row["config"], row["mode"]), {})
            for name in ("psnr", "sharpness", "ghosting"):
                group.setdefault(name, []).append(row[name])
        for row in result["flicker"]:
            group = groups.setdefault((result["scene"], row["config"], row["mode"]), {})
            group.setdefault("flicker", []).append(row["mean"])
    rows = []
    for (scene, config, mode), values in sorted(groups.items()):
        row = {"scene": scene, "config": config, "mode": mode,
               "snapshots": len(values.get("psnr", []))}
        for name, samples in values.items():
            row[name] = round(float(np.mean(samples)), 4)
        rows.append(row)
    return rows


def describe_average(rows: list[dict]) -> str:
    lines = ["averaged over the reports' snapshots:"]
    for row in rows:
        scores = "  ".join(f"{name} {row[name]:.3f}" for name in
                           ("psnr", "sharpness", "ghosting", "flicker") if name in row)
        lines.append(f"  {row['scene']:28} {row['config']:10} {row['mode']:11} "
                     f"n={row['snapshots']}  {scores}")
    return "\n".join(lines)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("report", type=Path, nargs="+",
                        help="one or more fast-report.json files; with more than one, the scores "
                             "are also averaged per scene over all of them")
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--session", type=int, help="the session whose captures the report saved")
    target.add_argument("--captures", type=Path, help="the captures folder the report saved into")
    parser.add_argument("--json", type=Path, help="also write the measurements here")
    args = parser.parse_args(argv)
    captures = args.captures or SESSIONS / f"persistent-{args.session}/config/captures"
    try:
        results = []
        for path in args.report:
            report = json.loads(path.read_text())
            results += [measure_scene(scene, captures) for scene in report.get("scenes", [])]
    except (OSError, ValueError, KeyError) as error:
        print(f"fsr-quality: {error}", file=sys.stderr)
        return 2
    for result in results:
        print(describe(result))
    averaged = average(results) if len(args.report) > 1 else None
    if averaged is not None:
        print(describe_average(averaged))
    if args.json:
        output = results if averaged is None else {"scenes": results, "average": averaged}
        args.json.write_text(json.dumps(output, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
