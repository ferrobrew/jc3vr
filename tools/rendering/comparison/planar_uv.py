import argparse
import json
from pathlib import Path

import numpy as np

from tools.rendering.capture.validation import validate_water_inputs, validate_water_treatment
from tools.rendering.protocol import Mode as ProtocolMode
from tools.rendering.comparison.images import Manifest, Mode, read_json
from tools.rendering.comparison.stages import (
    Source, Stage, artifact_path, decode_raw, load_stage, load_water_mask, validated_samples,
)


def mirror(uv):
    return 1 - np.abs(np.remainder(uv, 2) - 1)


def sample_map(texture, uv):
    h, w = texture.shape[:2]
    xy = np.clip(mirror(uv) * [w, h] - 0.5, 0, [w - 1, h - 1])
    x, y = np.floor(xy).astype(int).T
    x1, y1 = np.minimum(x + 1, w - 1), np.minimum(y + 1, h - 1)
    fx, fy = (xy - np.column_stack((x, y))).T[:, :, None]
    return ((1 - fy) * ((1 - fx) * texture[y, x] + fx * texture[y, x1]) +
            fy * ((1 - fx) * texture[y1, x] + fx * texture[y1, x1]))


def compare(a, b, projected):
    ma, ua, ta = a
    mb, ub, tb = b
    common = ma & mb
    if not common.any():
        raise ValueError('planar UV: no common selected-draw pixels')
    u, v = ua[common], ub[common]
    expected = u.copy()
    if projected:
        expected[:, 0] = 0.25 + u[:, 0] * 0.5
    target = sample_map(ta, u)
    observed = sample_map(tb, v)
    ideal = sample_map(tb, expected)
    addressed = mirror(u)
    if projected:
        addressed[:, 0] = 0.25 + addressed[:, 0] * 0.5
    addressed_color = sample_map(tb, addressed)
    outside_u = (u[:, 0] < 0) | (u[:, 0] > 1)
    def mae(x, y):
        return float(np.abs(x - y).mean())
    return dict(common=int(common.sum()), only_reference=int((ma & ~mb).sum()),
                only_comparison=int((mb & ~ma).sum()), uv_mae=np.abs(v - expected).mean(0).tolist(),
                uv_abs_p99=np.quantile(np.abs(v - expected), 0.99, axis=0).tolist(),
                reference_outside=int(np.any((u < 0) | (u > 1), axis=1).sum()),
                rgb_mae=mae(target, observed), ideal_rgb_mae=mae(target, ideal),
                mirror_before_projection_rgb_mae=mae(target, addressed_color), outside_u=int(outside_u.sum()),
                outside_u_ideal_rgb_mae=mae(target[outside_u], ideal[outside_u]) if outside_u.any() else None,
                outside_u_addressed_rgb_mae=mae(target[outside_u], addressed_color[outside_u]) if outside_u.any() else None,
                observed_vs_ideal_rgb_mae=mae(observed, ideal))


def analyze(manifest_path, regions_path):
    report = read_json(regions_path)
    if (Path(report['manifest']).resolve() != manifest_path.resolve() or report['status'] != 'measured' or report['errors']):
        raise ValueError('planar UV: missing eligible stage comparisons for manifest')
    localized, = report['cases']
    case, = Manifest.from_wire(read_json(manifest_path)).cases
    if localized['name'] != case.name:
        raise ValueError('planar UV: case mismatch')
    shots = {}
    root = manifest_path.parent
    for rows in case.modes:
        mode = rows.mode
        entries, errors = validated_samples(root, mode, rows.samples)
        if errors:
            raise ValueError(errors)
        for spec, frame in entries:
            path = root / spec.metadata
            wire = read_json(path)
            if wire.get('water_reflection') != 'planar_uv':
                raise ValueError(f'{path}: not a coordinate capture')
            validate_water_treatment(path.parent, frame.capture_id, wire, ProtocolMode(mode.value))
            validate_water_inputs(path.parent, frame.capture_id, wire, ProtocolMode(mode.value), True, 7)
            for eye in mode.eyes:
                buf = load_stage(root, mode, spec, frame, (Stage.WATER_AFTER_LEFT, Stage.WATER_AFTER_RIGHT)[eye], eye).values
                mask = load_water_mask(root, mode, spec, frame, eye).water
                signs = buf[..., 2] - 16
                marked = np.isin(signs, [0, 1, 2, 3])
                if not np.any(mask & marked):
                    raise ValueError(f'{path}: no tagged lake draw in water mask')
                uv = buf[..., :2] * np.where(signs.astype(int)[..., None] & [1, 2], -1, 1)
                rec, = [r for r in wire['stages'] if r['stage'] == 'water_input_static_reflection' and r['eye'] == eye]
                tex = decode_raw(artifact_path(path, rec['raw']).read_bytes(), Source.from_wire(rec['source']))[1]
                shots[frame.capture_id, eye] = (marked & mask, uv, tex)
    result = []
    for row in localized['repeatability'] + localized['comparisons']:
        eye = row['eye']
        if row['stage'] != ('water_after_left', 'water_after_right')[eye]:
            continue
        for pair in row['pairs']:
            a, b = (pair[key]['capture_id'] for key in ('a', 'b'))
            result.append(dict(comparison=row['name'], eye=eye, a=a, b=b,
                               **compare(shots[a, eye], shots[b, eye],
                                         row['modes'][-1] == Mode.SINGLE_PASS and len(set(row['modes'])) > 1)))
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('manifest', type=Path)
    parser.add_argument('regional_report', type=Path)
    args = parser.parse_args()
    print(json.dumps(analyze(args.manifest, args.regional_report), indent=2, allow_nan=False))
