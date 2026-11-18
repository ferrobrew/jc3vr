import argparse
import json
from pathlib import Path

import numpy as np

from tools.rendering.comparison.images import Manifest, Mode, read_json
from tools.rendering.comparison.stages import Stage, artifact_path, load_stage, validated_samples


DEPTH = ('before_z_pass', 'after_z_pass', 'after_z_velocity', 'before_tess_patch', 'after_tess_patch',
         'before_dynamic_models', 'after_dynamic_models', 'after_static_models', 'after_opaque_vegetation',
         'after_fins', 'after_group', 'after_group2', 'after_forest', 'geometry')


def load_planes(root, mode, spec, shot, eye):
    path = root / spec.metadata
    meta = read_json(path)
    w, h = meta['eye_width'], meta['eye_height']
    packed = mode == Mode.SINGLE_PASS
    def raw(stage):
        (r,) = [r for r in meta['stages'] if (r['stage'], r['eye']) == (stage, 0 if packed else eye)]
        s = r['source']
        if r['frame'] != shot.predicted_display_time_ns or r['error'] or (s['width'],s['height']) != (w*(1+packed), h):
            raise ValueError(f'{path}: invalid {stage} frame or dimensions')
        data = np.fromfile(artifact_path(path, r['raw']), np.uint8).reshape(h, s['width'], s['bytes_per_pixel'])
        return np.ascontiguousarray(np.split(data, 2, axis=1)[eye]) if packed else data
    boundaries = DEPTH
    if any(r['stage'] == 'after_base_near_depth' for r in meta['stages']):
        i = DEPTH.index('before_dynamic_models')
        boundaries = DEPTH[:i] + ('after_base_near', 'after_tess_far', 'after_base_far', 'after_tess_color', 'after_base_color', 'after_apply_near_detailed', 'after_apply_near') + DEPTH[i:]
    depth = [raw(stage+'_depth')[..., :4].copy().view('<u4')[..., 0] for stage in boundaries]
    before = raw('water_depth_before')[...,:4].copy().view('<f4')[...,0]
    after = raw('water_depth_after_'+('left','right')[eye])[...,:4].copy().view('<f4')[...,0]
    owners = None
    if any(r['stage'] == 'patch_depth_owners' for r in meta['stages']):
        owners = raw('patch_depth_owners').copy().view('<u4')[...,0]
        if owners.max() > 256: raise ValueError('terrain owner exceeds observed draw bound')
    return dict(id=shot.capture_id, owners=owners, boundaries=boundaries, depth=depth, gbuffer=[raw(f'gbuffer{i}') for i in range(4)],
                stencil=raw('geometry_depth')[...,4], region=np.where(after>before,2,np.where((before==0)&(after==0),1,0)),
                final=shot.image(eye))


def compare(a,b,threshold):
    diff=np.abs(a['final'].astype(int)-b['final']); changed=diff.max(2)>threshold
    if a['boundaries'] != b['boundaries']: raise ValueError('depth boundary mismatch')
    boundaries = a['boundaries']
    ds=np.stack([x!=y for x,y in zip(a['depth'],b['depth'],strict=True)])
    geom=a['gbuffer']; codes=sum(np.any(x!=y,axis=2).astype('u1')<<i for i,(x,y) in enumerate(zip(geom,b['gbuffer'],strict=True)))
    codes |= ds[-1].astype('u1')<<4; codes |= (a['stencil']!=b['stencil']).astype('u1')<<5
    common=(a['region']==0)&(b['region']==0); mask=changed&common
    last_equal=len(boundaries)-1-np.argmax(~ds[::-1],axis=0)
    first_persistent=np.where(ds.all(0),0,np.minimum(last_equal+1,len(boundaries)-1))
    tags=np.where(ds[-1],first_persistent,-1)
    def count(m):return int(m.sum())
    regions={'common_geometry':common, 'common_water':(a['region']==2)&(b['region']==2), 'common_sky':(a['region']==1)&(b['region']==1), 'disagreement':a['region']!=b['region']}
    row=dict(a=a['id'],b=b['id'],regions={k:dict(pixels=count(m),changed=count(changed&m),rgb_abs_sum=int(diff[m].sum())) for k,m in regions.items()},
             common_geometry_changed_by_plane_bits={int(k):count(mask&(codes==k)) for k in np.unique(codes[mask])},
             common_geometry_changed_by_depth_boundary={boundaries[int(k)] if k>=0 else 'equal_final_depth':count(mask&(tags==k)) for k in np.unique(tags[mask])})
    return row,mask,tags,diff.sum(2)


def sky_trace(root, case, group, strict, eye, threshold):
    stages = [Stage.POST_RESOLVE, Stage.POST_SUN, Stage.PRE_ATMOSPHERE, Stage.POST_ATMOSPHERE, Stage.WATER_BEFORE,
              (Stage.WATER_AFTER_LEFT, Stage.WATER_AFTER_RIGHT)[eye], Stage.AFTER_APPLY_CLOUDS,
              Stage.BEFORE_FOG_VOLUME_APPLY, Stage.AFTER_FOG_VOLUME_APPLY, Stage.MAIN_COLOR, Stage.DOF_SCENE,
              Stage.DOF_COMPOSITE, Stage.DOF_RETURNED, Stage.PRE_AA, Stage.POST_AA]
    data = {}
    for m in case.modes:
        if m.mode not in (group, Mode.SINGLE_PASS):
            continue
        entries, errors = validated_samples(root, m.mode, m.samples)
        if errors:
            raise ValueError(errors)
        for spec, shot in entries:
            def load(stage):
                return load_stage(root, m.mode, spec, shot, stage, eye).values
            sky = (load(Stage.WATER_DEPTH_BEFORE)[..., 0] == 0) & (load((Stage.WATER_DEPTH_AFTER_LEFT, Stage.WATER_DEPTH_AFTER_RIGHT)[eye])[..., 0] == 0)
            data[shot.capture_id] = (sky, shot.image(eye), [load(s)[..., :3].astype(np.float32) for s in stages])
    persistent = np.logical_and.reduce([v[0] for v in data.values()])
    for pair in strict['pairs']:
        a, b = (data[pair[k]][1] for k in ('capture_a', 'capture_b'))
        persistent &= abs(a.astype(int) - b).max(2) > threshold
    output = []
    y, x = np.nonzero(persistent)
    for pair in strict['pairs']:
        a, b = (data[pair[k]] for k in ('capture_a', 'capture_b'))
        measured = {}
        for name, u, v in zip(stages, a[2], b[2], strict=True):
            delta = abs(u[persistent] - v[persistent]).max(1)
            measured[name] = dict(nonzero=int((delta > 0).sum()), maximum=float(delta.max(initial=0)),
                                  above_three=int((delta > threshold).sum()) if name in stages[-4:] else None)
        output.append(dict(a=pair['capture_a'], b=pair['capture_b'], stages=measured))
    return dict(eye=eye, persistent_pixels=list(zip(x.tolist(), y.tolist())), pairs=output)


def analyze(manifest_path,report_path,case_name, *, sky=False):
    manifest=Manifest.from_wire(read_json(manifest_path)); report=read_json(report_path)
    if report['errors'] or Path(report['manifest']).resolve()!=manifest_path.resolve(): raise ValueError('invalid strict report')
    (case,)=[c for c in manifest.cases if c.name==case_name]
    (strict,)=[c for c in report['cases'] if c['name']==case_name]
    out=[]
    for eye,mode in enumerate((Mode.LEFT_ONLY,Mode.RIGHT_ONLY)):
        (group,)=[c for c in strict['comparisons'] if c['name']==f'{mode} vs single_pass, eye {eye}']
        if group['status'] not in ('pass','fail') or group['errors']:raise ValueError('physical comparison ineligible')
        if sky:
            out.append(sky_trace(manifest_path.parent, case, mode, group, eye, report['thresholds']['pixel']))
            continue
        planes={}
        for m in case.modes:
            if m.mode not in (Mode.SINGLE_PASS,mode):continue
            entries,errors=validated_samples(manifest_path.parent,m.mode,m.samples)
            if errors:raise ValueError(errors)
            for spec,shot in entries:planes[shot.capture_id]=load_planes(manifest_path.parent,m.mode,spec,shot,eye)
        rows=[];persistent=None;stable=None;first_tags=None;minimum=None
        for pair in group['pairs']:
            row,mask,tags,mass=compare(planes[pair['capture_a']],planes[pair['capture_b']],report['thresholds']['pixel'])
            rows.append(row)
            if persistent is None:persistent=mask.copy();first_tags=tags.copy();stable=np.ones_like(mask);minimum=mass
            else:persistent &= mask;stable &= tags==first_tags;minimum=np.minimum(minimum,mass)
        if not rows:raise ValueError('no paired captures')
        boundary={}
        boundaries = next(iter(planes.values()))['boundaries']
        for k in np.unique(first_tags[persistent&stable]):
            sel=persistent&stable&(first_tags==k);y,x=np.nonzero(sel);order=np.argsort(-minimum[y,x],kind='stable')[:12]
            name = boundaries[k] if k>=0 else 'equal_final_depth'
            boundary[name]=dict(pixels=int(sel.sum()),
                sum_of_min_rgb_abs=int(minimum[sel].sum()),
                top=[dict(x=int(x[i]),y=int(y[i]),min_rgb_abs_sum=int(minimum[y[i],x[i]])) for i in order])
            if name == 'after_tess_patch' and all(p['owners'] is not None for p in planes.values()):
                owners = {}
                for r in rows:
                    a,b = (planes[r[key]]['owners'][sel] for key in ('a','b'))
                    codes = a*257+b
                    owners[r['a']+' vs '+r['b']] = [dict(reference=int(c//257),sps=int(c%257),pixels=int((codes==c).sum()),
                        min_rgb_abs_sum=int(minimum[sel][codes==c].sum())) for c in np.unique(codes)]
                boundary[name]['last_writers_by_pair'] = owners
        out.append(dict(case=case.name,eye=eye,pairs=rows,persistent=int(persistent.sum()),stable_depth_boundary=boundary))
    return out


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('report',type=Path);p.add_argument('case');p.add_argument('--sky', action='store_true');args=p.parse_args()
    print(json.dumps(analyze(args.manifest,args.report,args.case,sky=args.sky),indent=2))
