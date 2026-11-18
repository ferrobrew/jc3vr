import argparse
import json
from itertools import combinations, product
from pathlib import Path

import numpy as np

from tools.rendering.capture.validation import validate_resolve_inputs
from tools.rendering.comparison.images import Manifest, Mode, read_json
from tools.rendering.protocol import CascadeScale, Mode as CaptureMode
from tools.rendering.comparison.stages import artifact_path, validated_samples


def load(root, mode, spec, shot, eye, *, with_view=False):
    path = root / spec.metadata
    metadata = read_json(path)
    packed = mode == Mode.SINGLE_PASS
    w, h = metadata['eye_width'], metadata['eye_height']

    def plane(stage, *, traced=False):
        key = eye if traced or not packed else 0
        (r,) = [r for r in metadata['stages'] if (r['stage'], r['eye']) == (stage, key)]
        if r['error'] or r['frame'] != shot.predicted_display_time_ns:
            raise ValueError(f'{path}: invalid {stage} frame')
        width = w * (1 + packed)
        s = r['source']
        if (s['width'], s['height']) != (width, h):
            raise ValueError(f'{path}: unexpected {stage} dimensions')
        data = np.fromfile(artifact_path(path, r['raw']), dtype='<f4' if traced else 'u1')
        data = data.reshape(h, width, 4 if traced else s['bytes_per_pixel'])
        if packed:
            halves = np.split(data, 2, 1)
            if traced and np.any(halves[1-eye]):
                raise ValueError(f'{path}: {stage} escaped its physical eye')
            data = halves[eye]
        if traced and (s['format_id'] != 2 or not np.isfinite(data).all()):
            raise ValueError(f'{path}: invalid trace format or values')
        return data

    full = plane('resolve_full_samples', traced=True)
    terms = plane('resolve_shadow_terms', traced=True)
    tap = plane('resolve_shadow_tap', traced=True)
    live = plane('resolve_world', traced=True)[..., 3] == 1
    if not np.array_equal(np.minimum(terms[..., 0], terms[..., 3])[live], full[..., 3][live]):
        raise ValueError(f'{path}: recorded operands do not reproduce shadow minimum')
    geom = np.concatenate([plane('geometry_depth')[..., :5], *(plane('gbuffer'+str(i)) for i in range(4))], axis=2)
    data = dict(id=shot.capture_id, terms=terms, tap=tap, live=live, shadow=full[..., 3], geom=geom, final=shot.image(eye))
    if with_view:
        data['view'] = plane('resolve_view', traced=True)
    return data


def compare(a, b, threshold):
    both = a['live'] & b['live']
    same = np.all(a['geom'] == b['geom'], axis=2)
    shadow = a['shadow'] != b['shadow']
    delta = np.abs(a['final'].astype(int) - b['final'])
    active = (a['terms'][..., 0] > 0) & (b['terms'][..., 0] > 0)
    mask = both & same & shadow & active
    def count(m):
        return int(np.count_nonzero(m))
    d = a['terms'] != b['terms']
    coords = a['tap'] != b['tap']
    return dict(a=a['id'], b=b['id'], common_resolve=count(both), final_mae=float(delta.mean()),
                final_changed=count(delta.max(2)>threshold), same_geometry=count(both & same),
                shadow_diff=count(both & shadow), same_geom_shadow_diff=count(both & same & shadow),
                same_geom_shadow_final_changed=count(both & same & shadow & (delta.max(2)>threshold)),
                shadow_diff_both_active=count(mask), horizon_changed=count(mask & d[..., 0]),
                filter_changed=count(mask & d[..., 3]), primary_mean_changed=count(mask & d[..., 2]),
                first_pcf_changed=count(mask & d[..., 1]), uv_changed=count(mask & coords[..., :2].any(2)),
                slice_changed=count(mask & coords[..., 2]), ref_changed=count(mask & coords[..., 3]),
                first_result_only=count(mask & ~coords.any(2) & d[..., 1]))


def analyze(manifest_path, report_path):
    manifest = Manifest.from_wire(read_json(manifest_path))
    report = read_json(report_path)
    if Path(report['manifest']).resolve() != manifest_path.resolve() or report['errors']:
        raise ValueError('shadow comparison: wrong strict report or invalid manifest')
    for case, result in zip(manifest.cases, report['cases'], strict=True):
        if case.name != result['name']:
            raise ValueError('shadow comparison: case mismatch')
        modes = {m['mode']: m for m in result['modes']}
        for eye in (0,1):
            shots = {}
            for group in case.modes:
                if eye not in group.mode.eyes:
                    continue
                entries, errors = validated_samples(manifest_path.parent, group.mode, group.samples)
                if errors:
                    raise ValueError(errors)
                shots[group.mode] = [load(manifest_path.parent, group.mode, spec, shot, eye) for spec, shot in entries]
            isolated = (Mode.LEFT_ONLY, Mode.RIGHT_ONLY)[eye]
            for a,b in [(Mode.SINGLE_PASS,Mode.SINGLE_PASS), (isolated,Mode.SINGLE_PASS), (Mode.MULTIPASS,Mode.SINGLE_PASS)]:
                group = (next(r for r in modes[a]['repeatability'] if r['name']==f'{a} eye {eye} repeatability') if a==b else
                         next(r for r in result['comparisons'] if r['name']==f'{a} vs {b}, eye {eye}'))
                pairs = combinations(shots[a],2) if a==b else product(shots[a],shots[b])
                for x,y in pairs:
                    yield dict(case=case.name, eye=eye, comparison=group['name'], strict_group_status=group['status'],
                               **compare(x,y,report['thresholds']['pixel']))


def primary_tap(view, cb):
    def mad(a, b, c):
        return (a.astype(np.float64) * b + c).astype(np.float32)

    xyz = mad(view[..., :1], cb[45, :3], view[..., 1:2] * cb[46, :3])
    xyz = mad(view[..., 2:3], cb[47, :3], xyz) + cb[48, :3]
    coords = np.full_like(xyz, -1)
    radius = np.full(view.shape[:2], -1, dtype=np.float32)
    index = np.full_like(radius, -1)
    selector = np.full(radius.shape, -1, dtype=np.int8)
    for k in reversed(range(6)):
        scale, layer, gain, bias = cb[51+k]
        box = mad(xyz, np.array([scale, scale, 1], dtype=np.float32), cb[57+k, :3])
        inside = mad(np.abs(box[..., :2]).max(2), gain, bias) < 1
        coords[inside] = box[inside]
        radius[inside] = cb[57+k, 3]
        index[inside] = layer
        selector[inside] = k
    result = np.empty_like(view)
    result[..., :2] = mad(coords[..., :2], np.array([0.5, -0.5]), 0.5)
    result[..., :2] = mad((radius * cb[49, 2])[..., None], np.array([-0.3990621566772461, 0.9420162439346313]), result[..., :2])
    result[..., 2] = index
    result[..., 3] = coords[..., 2].clip(0, 1) - np.float32(2.1194999135332182e-5)
    return result, selector


def replay_row_changes(a, b, marked):
    mask = (a['live'] & b['live'] & (a['terms'][..., 0] > 0) & (b['terms'][..., 0] > 0) &
            (a['selector'] == 3) & (b['selector'] == 3) & (a['shadow'] != b['shadow']) & marked)
    result = {'pixels': int(mask.sum())}
    if not mask.any():
        return result
    view = a['view'][mask][None, ...]
    actual = b['tap'][mask][:, [0, 1, 3]]
    for name, changes in (
            ('original', []), ('scale', [(54, 0)]), ('offset', [(60, slice(3))]),
            ('scale_offset', [(54, 0), (60, slice(3))]),
            ('base_scale_offset', [(slice(45, 49), slice(3)), (54, 0), (60, slice(3))])):
        cb = a['cb'].copy()
        for index in changes:
            cb[index] = b['cb'][index]
        tap, selector = primary_tap(view, cb)
        diff = tap[0][:, [0, 1, 3]] - actual
        result[name] = dict(exact_uv=int((diff[:, :2] == 0).all(1).sum()), exact_uv_ref=int((diff == 0).all(1).sum()),
                            max_abs=abs(diff).max(0).tolist(), different_selector=int((selector != 3).sum()))
    return result


def repeat_cascades(manifest_path, report_path):
    manifest = Manifest.from_wire(read_json(manifest_path))
    report = read_json(report_path)
    if Path(report['manifest']).resolve() != manifest_path.resolve() or report['errors']:
        raise ValueError('shadow repeatability: incompatible strict report')
    captures = {}
    pairs = []
    seen = set()
    for case, result in zip(manifest.cases, report['cases'], strict=True):
        if case.name != result['name']:
            raise ValueError('shadow repeatability: case mismatch')
        for group in case.modes:
            strict, = (m for m in result['modes'] if m['mode'] == group.mode)
            entries, errors = validated_samples(manifest_path.parent, group.mode, group.samples)
            if errors:
                raise ValueError(errors)
            for eye in group.mode.eyes:
                signature = (group.mode, eye, tuple((s.metadata, s.png) for s, _ in entries))
                if signature in seen:
                    continue
                seen.add(signature)
                peers, = (r for r in strict['repeatability'] if r['name'] == f'{group.mode} eye {eye} repeatability')
                strict_pairs = {(p['capture_a'], p['capture_b']): p for p in peers['pairs']}
                data = []
                for spec, shot in entries:
                    path = manifest_path.parent / spec.metadata
                    wire = read_json(path)
                    validate_resolve_inputs(path.parent, shot.capture_id, wire, CaptureMode(group.mode), True)
                    obs = wire['resolve_inputs']['eyes'][eye]['observation']
                    ident = obs['pixel_shader']['identity']['identity']
                    b = obs['constant_buffers'][0]
                    if (ident['pristine_sha256'] != '06f43db0887b0038147e273a3879a9b7f48c3b630955a94d695898fd7d0e52fc' or
                            ident['actual_sha256'] != '6a82533e6458166ea3d04789c266ec127baf59931882955c99c4603e47f11c9b' or
                            (b['first_constant'], b['num_constants'], b['byte_width']) != (0, 95, 1520)):
                        raise ValueError(f'{path}: unexpected resolve program or CB0')
                    cb = np.fromfile(artifact_path(path, b['content']['raw']), dtype='<f4').reshape(95, 4)
                    level, = [s for s in CascadeScale if s.bits == cb[54, 0].view('<u4')]
                    d = load(manifest_path.parent, group.mode, spec, shot, eye, with_view=True)
                    tap, selector = primary_tap(d['view'], cb)
                    active = d['live'] & (d['terms'][..., 0] > 0)
                    error = np.abs(tap[active] - d['tap'][active]).max(0)
                    if error[2] or max(error[0], error[1], error[3]) > 1e-6:
                        raise ValueError(f'{path}: primary tap replay mismatch: {error}')
                    d.update(selector=selector, level=level, cb=cb)
                    data.append(d)
                    captures.setdefault(shot.capture_id, {})[eye] = dict(level=level, counters=obs['frame_counters'], active=int(active.sum()), max_abs_model_error=error.tolist())
                for a, b in combinations(data, 2):
                    p = strict_pairs.pop((a['id'], b['id']))
                    threshold = report['thresholds']['pixel']
                    diff = compare(a, b, threshold)
                    pixels = a['live'].size
                    if (diff['final_changed'] != round(p['metrics']['changed_fraction'] * pixels) or
                            abs(diff['final_mae'] - p['metrics']['mean']) > 1e-12):
                        raise ValueError('shadow repeatability: final comparison disagrees with strict report')
                    marked = np.abs(a['final'].astype(int) - b['final']).max(2) > threshold
                    active = a['live'] & b['live'] & (a['terms'][..., 0] > 0) & (b['terms'][..., 0] > 0)
                    shadow = active & (a['shadow'] != b['shadow'])
                    pairs.append(dict(case=case.name, mode=group.mode, eye=eye, status=p['status'], levels=[a['level'], b['level']],
                                      changed_cb_rows=np.flatnonzero((a['cb'].view('<u4') != b['cb'].view('<u4')).any(1)).tolist(),
                                      active_selector_changes=int((active & (a['selector'] != b['selector'])).sum()),
                                      row54_model_a_to_b=replay_row_changes(a, b, marked), row54_model_b_to_a=replay_row_changes(b, a, marked),
                                      changed_shadow_and_final_by_primary_row={k: int((shadow & marked & (a['selector'] == k) & (b['selector'] == k)).sum()) for k in range(-1, 6)}, **diff))
                if strict_pairs:
                    raise ValueError('shadow repeatability: incomplete pair list')
    return dict(captures=captures, pairs=pairs)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Descriptive comparisons of measured resolve shadow operands; does not alter strict eligibility.')
    parser.add_argument('manifest', type=Path)
    parser.add_argument('report', type=Path)
    parser.add_argument('--repeat-cascades', action='store_true')
    args = parser.parse_args()
    result = repeat_cascades(args.manifest, args.report) if args.repeat_cascades else list(analyze(args.manifest, args.report))
    print(json.dumps(result, indent=2, allow_nan=False))
