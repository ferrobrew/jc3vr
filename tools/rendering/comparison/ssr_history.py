import argparse
import json
from functools import lru_cache
from itertools import product
from pathlib import Path

import numpy as np

from tools.rendering.comparison.ssr_march import coordinates
from tools.rendering.comparison.stages import Source, artifact_path, decode_raw


@lru_cache(maxsize=36)
def observation(path, eye):
    meta = json.loads(path.read_text())
    packed = meta['mode'] == 'single_pass'
    w, h = meta['eye_width'] // 2, meta['eye_height'] // 2
    def plane(name, fmt):
        r, = (r for r in meta['stages'] if r['stage'] == name and r['eye'] == eye)
        src = r['source']
        if (r['frame'] != meta['predicted_display_time_ns'] or r['error'] or
                (src['width'], src['height'], src['format_id']) != (w*(1+packed), h, fmt)):
            raise ValueError(f'{path}: invalid {name} eye {eye}')
        data = artifact_path(path, r['raw']).read_bytes()
        if fmt == 26:
            return decode_raw(data, Source.from_wire(src))[1]
        a = np.frombuffer(data, '<f4').reshape(h, src['width'], -1)
        return a[:, eye*w:(eye+1)*w] if packed else a
    tex = plane('ssr_history_source', 26)
    uv = plane('ssr_history_uv', 2)
    rgb = plane('ssr_history_sample', 2)
    alpha = plane('ssr_ray_alpha', 41)[..., 0]
    hit = uv[..., 3] == 1
    if not np.array_equal(hit, rgb[..., 3] == 1) or not np.array_equal(uv[..., 2], alpha):
        raise ValueError(f'{path}: history checkpoints disagree with native alpha or hit marker')
    y, x, edge = coordinates(uv[hit, :2], tex.shape[:2])
    bad = np.any(tex[y,x] != rgb[hit, :3], axis=1)
    return meta['id'], (tex, uv, rgb, alpha), dict(hits=int(hit.sum()), lookup_mismatch=int(bad.sum()),
        interior_mismatch=int((bad & (edge>.001)).sum()), interior=int((edge>.001).sum()))


def pair(a,b,eye):
    at, au, ar, aa = a
    bt, bu, br, ba = b
    w = at.shape[1]
    half = bt[:, eye*w:(eye+1)*w]
    def metrics(mask):
        a_uv, b_uv = au[mask,:2], bu[mask,:2]
        actual_a, actual_b = ar[mask,:3], br[mask,:3]
        ay,ax,_ = coordinates(a_uv,at.shape[:2])
        by,bx,_ = coordinates(b_uv,bt.shape[:2])
        py,px,_ = coordinates(a_uv,half.shape[:2])
        physical = half[py,px]
        uv_expected = a_uv.copy()
        uv_expected[:,0] = (a_uv[:,0]+eye)/2
        def mae(d):
            return float(np.abs(d).mean()) if len(d) else None
        return dict(count=int(mask.sum()), actual_rgb_mae=mae(actual_b-actual_a),
            corresponding_source_mae=mae(physical-actual_a),
            uv_error_vs_packed_iso=mae(b_uv-uv_expected),
            actual_uv_mean=b_uv.mean(0).tolist() if len(b_uv) else [],
            same_texel=int(((by==ay)&(bx==ax+w*eye)).sum()), wrong_half=int(((bx>=w)!=bool(eye)).sum()),
            equal_sample=int(np.all(actual_b==actual_a,axis=1).sum()))
    return dict(common_hit=metrics((au[...,3]==1)&(bu[...,3]==1)), common_nonzero=metrics((aa!=0)&(ba!=0)),
                corresponding_texture_mae=float(np.abs(half-at).mean()))


def analyze(manifest):
    checks, pairs = {}, []
    for case in json.loads(manifest.read_text())['cases']:
        for eye, iso in enumerate(('left_only', 'right_only')):
            for mode in (iso,'multipass','single_pass'):
                for s in case['modes'][mode]:
                    cid,_,check = observation(manifest.parent/s['metadata'],eye)
                    checks[cid,eye] = check
            for sa,sb in product(case['modes'][iso],case['modes']['single_pass']):
                ca,a,_ = observation(manifest.parent/sa['metadata'],eye)
                cb,b,_ = observation(manifest.parent/sb['metadata'],eye)
                pairs.append(dict(case=case['name'], eye=eye, a=ca, b=cb, **pair(a,b,eye)))
    return dict(checks=[dict(id=k[0],eye=k[1],**v) for k,v in checks.items()],pairs=pairs)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('manifest', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    data = analyze(args.manifest)
    with args.output.open('x') as file:
        json.dump(data,file,indent=2)
        file.write('\n')
