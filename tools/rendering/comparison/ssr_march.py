import argparse
import json
from functools import lru_cache
from itertools import product
from pathlib import Path

import numpy as np

from tools.rendering.comparison.stages import artifact_path


@lru_cache(maxsize=32)
def load(path, eye):
    meta = json.loads(path.read_text())
    frame = meta['predicted_display_time_ns']
    packed = meta['mode'] == 'single_pass'
    w, h = meta['eye_width'] // 2, meta['eye_height'] // 2
    full = w * (1 + packed)
    records = [r for r in meta['stages'] if r['eye'] == eye]
    planes = {}
    for name in ('prep_depth', 'prep_plane', 'prep_scene', 'ray_input', 'ray_world', 'ray_normal',
                 'ray_segment', 'ray_march', 'ray_mask', 'ray_alpha'):
        record, = [r for r in records if r['stage'] == 'ssr_' + name]
        src = record['source']
        if (record['error'] or record['frame'] != frame or
                (src['width'], src['height']) != (full, h) or
                src['format_id'] != (19 if name == 'prep_scene' else 41 if name.startswith('prep') or name == 'ray_alpha' else 2)):
            raise ValueError(f"{path}: invalid {name} eye {eye}")
        plane = np.fromfile(artifact_path(path, record['raw']), '<f4').reshape(h, full, src['bytes_per_pixel'] // 4)
        if name == 'prep_depth':
            planes['texture'] = plane[..., 0]
        plane = plane[:, w*eye:w*(eye+1)] if packed else plane
        planes[name] = plane[..., 0] if name.startswith('prep') or name == 'ray_alpha' else plane
    return meta['id'], planes


def coordinates(uv, shape):
    h, w = shape
    xy = uv.astype('d') * [w, h]
    ix = np.floor(xy).astype(int).clip([0, 0], [w-1, h-1])
    return ix[:, 1], ix[:, 0], np.min(np.abs(xy - np.rint(xy)), axis=1)


def sample_check(s):
    m = s['ray_march']
    m = m[(s['ray_input'][..., 3] == 1) & (m[..., 2] != 0)]
    tex = s['texture']
    y, x, edge = coordinates(m[:, :2], tex.shape)
    bad = tex[y, x] != m[:, 2]
    interior = edge > 0.001
    return dict(samples=len(x), floor_mismatch=int(bad.sum()), interior=int(interior.sum()),
                interior_mismatch=int((bad & interior).sum()),
                max_edge_distance_of_mismatch=float(edge[bad].max(initial=0)))


def compare(a, b, eye):
    gate = (a['ray_input'][..., 3] == 1) & (b['ray_input'][..., 3] == 1)
    count = np.count_nonzero
    def delta(name, cols, mask):
        v = np.abs(a[name][mask][:, cols].astype('d') - b[name][mask][:, cols])
        return dict(different=int(count(v.any(1))), mean_abs=float(v.mean()) if len(v) else None,
                    max_abs=float(v.max(initial=0)))
    setup = gate & (a['ray_input'][..., 2] == b['ray_input'][..., 2]) & np.all(a['ray_normal'] == b['ray_normal'], axis=2)
    march = setup & (a['ray_march'][..., 2] != 0) & (b['ray_march'][..., 2] != 0)
    am, bm = (v['ray_march'][march] for v in (a, b))
    ay, ax, ae = coordinates(am[:, :2], a['texture'].shape)
    by, bx, be = coordinates(bm[:, :2], b['texture'].shape)
    cy, cx, ce = coordinates(bm[:, :2], b['prep_depth'].shape)
    interior = np.minimum.reduce([ae, be, ce]) > .001
    raw = b['texture'][by, bx]
    corrected = b['prep_depth'][cy, cx]
    different = am[:, 2] != bm[:, 2]
    proxy_a, proxy_b = a['prep_plane'], b['prep_plane']
    pd = proxy_a != proxy_b
    ca, cb = proxy_a != 0, proxy_b != 0
    da, db = proxy_a > a['prep_scene'], proxy_b > b['prep_scene']
    return dict(shared=int(gate.sum()), entry=delta('ray_input', [2], gate), normal=delta('ray_normal', slice(None), gate),
                equal_inputs=int(setup.sum()), world=delta('ray_world', slice(3), setup),
                segment_start=delta('ray_segment', slice(2), setup), segment_end=delta('ray_segment', slice(2,4), setup),
                both_first=int(march.sum()), first_uv=delta('ray_march', slice(2), march),
                first_sample_different=int(different.sum()), other_half=int(count((bx >= a['texture'].shape[1]) != bool(eye))),
                hit_presence_different=int(count(setup & ((a['ray_mask'][..., 3] == 1) != (b['ray_mask'][..., 3] == 1)))),
                interior=int(interior.sum()), interior_actual_depth_different=int(count(interior & different)),
                interior_native_lookup_mismatch=int(count(interior & (raw != bm[:, 2]))),
                interior_isolated_lookup_mismatch=int(count(interior & (a['texture'][ay, ax] != am[:, 2]))),
                interior_eye_lookup_differs_from_isolated=int(count(interior & (corrected != am[:, 2]))),
                interior_same_physical_texel=int(count(interior & (ax == cx) & (ay == cy))),
                proxy_different=int(pd.sum()), proxy_coverage=[int(count(pd & x)) for x in (ca & ~cb, cb & ~ca, ca & cb)],
                proxy_wins=[int(count(pd & x)) for x in (da & ~db, db & ~da, da & db, ~da & ~db)],
                proxy_different_scene_different=int(count(pd & (a['prep_scene'] != b['prep_scene']))))


def analyze(manifest):
    pairs, checks = [], {}
    for case in json.loads(manifest.read_text())['cases']:
        for eye, iso in enumerate(('left_only', 'right_only')):
            for sa, sb in product(case['modes'][iso], case['modes']['single_pass']):
                observations = []
                for spec in (sa, sb):
                    cid, planes = load(manifest.parent / spec['metadata'], eye)
                    if (cid, eye) not in checks:
                        checks[cid, eye] = sample_check(planes)
                    observations.append((cid, planes))
                (ca, a), (cb, b) = observations
                pairs.append(dict(case=case['name'], eye=eye, a=ca, b=cb, **compare(a,b,eye)))
    return dict(checks=[dict(id=c, eye=e, **v) for (c,e),v in checks.items()], pairs=pairs)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    for arg in ('manifest', 'output'):
        parser.add_argument(arg, type=Path)
    args = parser.parse_args()
    with args.output.open('x') as f:
        json.dump(analyze(args.manifest), f, indent=2)
        f.write('\n')
