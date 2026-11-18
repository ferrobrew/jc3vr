import argparse
import json
from pathlib import Path

import numpy as np

from tools.rendering.comparison.images import Manifest, Mode, read_json
from tools.rendering.comparison.stages import Stage, artifact_path, load_stage, validated_samples


def reconstruct(rgb, weights, eye_width):
    h, w = weights.shape[:2]
    y, x = np.indices((h, w))
    r, _, b, _ = weights.astype(np.int16).transpose(2, 0, 1)
    a = weights[y, (x+1).clip(0,w-1), 3].astype(np.int16)
    g = weights[(y+1).clip(0,h-1), x, 1].astype(np.int16)
    u, v = np.where(b < a, a, -b), np.where(r < g, g, -r)
    horizontal = abs(v) < abs(u)
    xx = (x + np.where(horizontal, np.sign(u), 0)).clip(0,w-1)
    yy = (y + np.where(horizontal, 0, np.sign(v))).clip(0,h-1)
    alpha = np.maximum(abs(u), abs(v))[..., None]/255
    s = rgb / 255
    linear = np.where(s <= .04045, s/12.92, ((s+.055)/1.055)**2.4)
    def blend(tx):
        t = (1-alpha)*linear + alpha*linear[yy,tx]
        return np.rint(255*np.where(t <= .0031308, 12.92*t, 1.055*t**(1/2.4)-.055)).astype(np.int16)
    low = (x // eye_width)*eye_width
    return blend(xx), blend(xx.clip(low,low+eye_width-1)), xx//eye_width != x//eye_width


def analyze(manifest_path, report_path, name):
    root = manifest_path.parent
    manifest = Manifest.from_wire(read_json(manifest_path))
    report = read_json(report_path)
    if Path(report['manifest']).resolve() != manifest_path.resolve() or report['errors']:
        raise ValueError('AA: invalid strict report')
    case, = [c for c in manifest.cases if c.name == name]
    strict, = [c for c in report['cases'] if c['name'] == name]
    shots = {}
    evidence = []
    for m in case.modes:
        entries, errors = validated_samples(root,m.mode,m.samples)
        if errors:
            raise ValueError(errors)
        for spec, shot in entries:
            path=root/spec.metadata
            wire=read_json(path)
            packed=m.mode == Mode.SINGLE_PASS
            for dispatch in ([0] if packed else m.mode.eyes):
                r, = [r for r in wire['stages'] if r['stage']=='aa_weights' and r['eye']==dispatch]
                eyes=m.mode.eyes if packed else (dispatch,)
                def stage(s):
                    return np.concatenate([load_stage(root,m.mode,spec,shot,s,e).values[..., :3] for e in eyes],axis=1)
                pre,post=stage(Stage.PRE_AA),stage(Stage.POST_AA)
                w,h=wire['eye_width'],wire['eye_height']
                if r['error'] or r['frame']!=shot.predicted_display_time_ns or (r['source']['width'],r['source']['height'],r['source']['format_id']) not in [(w*len(eyes),h,k) for k in (27,28)]:
                    raise ValueError('AA: invalid weights')
                weights=np.fromfile(artifact_path(path,r['raw']),np.uint8).reshape(h,w*len(eyes),4)
                native,local,cross=reconstruct(pre,weights,w)
                treated=wire.get('aa_color_edge') is True
                error=abs((local if treated else native)-post).max(2)
                if error.max()>1:
                    raise ValueError(f'AA: {shot.capture_id} replay error {error.max()}')
                for i,e in enumerate(eyes):
                    sl=np.s_[:,i*w:(i+1)*w]
                    evidence.append(dict(id=shot.capture_id,eye=e,cross=int(cross[sl].sum()),max_replay_codes=int(error[sl].max())))
                    shots[shot.capture_id,e]=(post[sl],local[sl],cross[sl])
    pairs=[]
    for eye,iso in enumerate((Mode.LEFT_ONLY,Mode.RIGHT_ONLY)):
        group, = [g for g in strict['comparisons'] if g['name']==f'{iso} vs single_pass, eye {eye}']
        for pair in group['pairs']:
            a,b=pair['capture_a'],pair['capture_b']; ref=shots[a,eye][0]; post,local,cross=shots[b,eye]
            above=lambda img:(abs(img-ref).max(2)>report['thresholds']['pixel']) & cross
            pairs.append(dict(a=a,b=b,eye=eye,strict_status=group['status'],cross=int(cross.sum()),observed_above=int(above(post).sum()),model_eye_local_above=int(above(local).sum())))
    return dict(case=name,captures=evidence,pairs=pairs)


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('report',type=Path);p.add_argument('case');a=p.parse_args()
    print(json.dumps(analyze(a.manifest,a.report,a.case),indent=2))
