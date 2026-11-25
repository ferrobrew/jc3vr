import argparse
import json
import re
from itertools import combinations
from pathlib import Path

import numpy as np

from tools.rendering.comparison.stages import artifact_path


def analyze(manifest, log):
    cases = json.loads(manifest.read_text())["cases"]
    rows = {}
    for cid, frame, eye, words in re.findall(
        r"SSR constants id=(\S+) frame=(\d+) eye=(\d+) n=0 slot=1 first=0 count=\d+ words=(\[[^\]]+\])", log.read_text()
    ):
        rows[cid, int(frame), int(eye)] = np.asarray(json.loads(words), '<u4').view('<f4')
    cache, checks, pairs = {}, {}, {}

    def load(spec, eye):
        key = spec['metadata'], eye
        if key in cache:
            return cache[key]
        path = manifest.parent / spec['metadata']
        m = json.loads(path.read_text()); cid = m['id']; frame = m['predicted_display_time_ns']
        records = {r['stage']: r for r in m['stages'] if r['eye'] == eye}
        width = m['eye_width']//2
        packed = m['mode'] == 'single_pass'

        def plane(stage, dtype):
            r = records['ssr_' + stage]; src = r['source']
            assert r['frame'] == frame and not r['error'] and src['width'] == width*(1+packed)
            return np.fromfile(artifact_path(path,r['raw']), dtype).reshape(src['height'],src['width'],-1)

        def crop(a):
            return a[:, eye*width:(eye+1)*width] if packed else a

        proxy = plane('prep_plane','<f4')[...,0]; scene = plane('prep_scene','<f4')[...,0]
        depth = plane('prep_depth','<f4')[...,0]; normal = plane('prep_normal','u1')
        a,b = rows[cid,frame,eye][:2]
        expected = np.where((proxy==0)&(scene==0),1.,1./(np.maximum(proxy,scene)*a+b))
        measured = crop(depth); calculated = crop(expected)
        inp = crop(plane('ray_input','<f4')); march = crop(plane('ray_march','<f4')); mask = crop(plane('ray_mask','<f4'))
        survivors = inp[...,3]==1; first = survivors & (march[...,2]!=0)

        def lookup(a,uv):
            h,w = a.shape
            return a[np.clip(np.floor(uv[...,1]*h).astype(int),0,h-1),np.clip(np.floor(uv[...,0]*w).astype(int),0,w-1)]

        checks[cid,eye] = dict(prep_max_abs=float(abs(measured-calculated).max()),
            prep_mean_abs=float(abs(measured-calculated).mean()),survivors=int(survivors.sum()),
            ray_entry_unequal=int((survivors & (lookup(depth,inp)!=inp[...,2])).sum()),
            march_nonzero_samples=int(first.sum()),march_sample_unequal=int((first & (lookup(depth,march)!=march[...,2])).sum()),
            march_other_eye=int((first & ((march[...,0]>=0.5)!=bool(eye))).sum()) if packed else None,
            water_hits=int((mask[...,3]==1).sum()), water_sample_unequal=int(((mask[...,3]==1) & (lookup(proxy,mask)!=mask[...,2])).sum()))
        result = dict(proxy=crop(proxy),scene=crop(scene),depth=measured,normal=crop(normal),gate=survivors,ray=crop(plane('ray_alpha','<f4'))[...,0]);cache[key]=cid,result
        return cid,result

    for case in cases:
        modes=case['modes']
        for eye,iso in enumerate(['left_only','right_only']):
            selections=[(mode,spec) for mode in (iso,'multipass','single_pass') for spec in modes[mode]]
            for (ma,sa),(mb,sb) in combinations(selections,2):
                ca,a=load(sa,eye);cb,b=load(sb,eye); key=ca,cb,eye
                if key in pairs: continue
                pa, pb = a['proxy']>a['scene'],b['proxy']>b['scene']; shared=a['gate']&b['gate'];diff=a['depth']!=b['depth'];union=(a['ray']!=0)|(b['ray']!=0)
                pairs[key]=dict(a_mode=ma,b_mode=mb,depth_diff=int(diff.sum()),proxy_diff=int((a['proxy']!=b['proxy']).sum()),scene_diff=int((a['scene']!=b['scene']).sum()),normal_diff=int(np.any(a['normal']!=b['normal'],axis=-1).sum()),branch_diff=int((pa!=pb).sum()),depth_diff_both_proxy=int((diff&pa&pb).sum()),depth_diff_both_scene=int((diff&~pa&~pb).sum()),shared_gate=int(shared.sum()),shared_gate_depth_diff=int((shared&diff).sum()),ray_union=int(union.sum()),union_diff_depth=int((union&diff).sum()),union_diff_proxy=int((union&(a['proxy']!=b['proxy'])).sum()),ray_diff=int((a['ray']!=b['ray']).sum()))
    return dict(checks=[dict(id=k[0],eye=k[1],**v) for k,v in checks.items()],pairs=[dict(a=k[0],b=k[1],eye=k[2],**v) for k,v in pairs.items()])


if __name__ == '__main__':
    p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('log',type=Path);p.add_argument('output',type=Path);args=p.parse_args()
    args.output.write_text(json.dumps(analyze(args.manifest,args.log),indent=2)+'\n')
