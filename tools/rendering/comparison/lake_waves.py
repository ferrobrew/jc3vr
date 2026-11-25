import argparse
import hashlib
import json
import re
from pathlib import Path

import numpy as np

from tools.rendering.capture.validation import GRADIENT_STAGES, WAVE_STAGES, validate_water_inputs, validate_water_treatment
from tools.rendering.comparison.images import Manifest, Mode, read_json
from tools.rendering.comparison.stages import Stage, artifact_path, load_stage, load_water_mask, validated_samples
from tools.rendering.protocol import Mode as ProtocolMode


def load(root, mode, spec, shot, eye):
    path = root / spec.metadata
    wire = read_json(path)
    if wire.get('lake_waves') is not True or wire.get('lake_normals') or wire.get('lake_displacement') is not True:
        raise ValueError(f'{path}: not a wave trace')
    validate_water_treatment(path.parent, shot.capture_id, wire, ProtocolMode(mode.value))
    validate_water_inputs(path.parent, shot.capture_id, wire, ProtocolMode(mode.value), True, 7, True)

    def record(name):
        rec, = [r for r in wire['stages'] if r['stage'] == name and r['eye'] == eye]
        if rec['frame'] != shot.predicted_display_time_ns or rec.get('error'):
            raise ValueError(f'{path}: invalid {name} frame or error')
        return rec

    def trace(name):
        rec = record(name)
        w, h = wire['eye_width'], wire['eye_height']
        width = w * (2 if mode == Mode.SINGLE_PASS else 1)
        raw = artifact_path(path, rec['raw'])
        if (rec['source'] != dict(width=width, height=h, format_id=2, format='DXGI_FORMAT(2)', bytes_per_pixel=16)
                or raw.name != f'{shot.capture_id}.{name}.eye{eye}.raw'):
            raise ValueError(f'{path}: invalid {name} output')
        arr = np.fromfile(raw, '<f4').reshape(h, width, 4)
        if mode == Mode.SINGLE_PASS:
            halves = np.split(arr, 2, axis=1)
            if np.any(halves[1-eye]):
                raise ValueError(f'{path}: {name} escaped physical eye')
            arr = halves[eye]
        if not np.isfinite(arr).all():
            raise ValueError(f'{path}: nonfinite {name}')
        return arr

    if wire.get('lake_gradients'):
        sample, uv, cx, output, cy, fx, fy = map(trace, GRADIENT_STAGES)
        mark = output[...,3]
        if not np.isin(mark, [0,1]).all() or any(np.any(arr[mark==0]) for arr in (sample, uv, cx, cy, fx, fy)):
            raise ValueError(f'{path}: invalid gradient coverage')
        mask = load_water_mask(root,mode,spec,shot,eye).water & (mark==1)
        indices = ('water_input_wave1','water_input_wave2')
        resources = [{k:record(n)['water_input'][k] for k in ('sampler','most_detailed_mip','mip_levels','wave_footprint')} for n in indices]
        shaded = load_stage(root,mode,spec,shot,(Stage.WATER_AFTER_LEFT,Stage.WATER_AFTER_RIGHT)[eye],eye).values[...,:3]
        return dict(mask=mask,output=output[...,:3],shaded=shaded,resources=resources,
                    **{k:v.reshape(*mask.shape,2,2) for k,v in zip(('sample','uv','cx','cy','fx','fy'),(sample,uv,cx,cy,fx,fy))})
    a, b, u, output, v, weights, combined = map(trace, WAVE_STAGES)
    mark = output[..., 3]
    if not np.isin(mark, [0, 1]).all() or any(np.any(arr[mark == 0]) for arr in (a, b, u, v, weights, combined)):
        raise ValueError(f'{path}: invalid selected draw coverage')
    mask = load_water_mask(root, mode, spec, shot, eye).water & (mark == 1)
    proj = record('water_input_static_reflection')['water_input']['projection']
    cb = np.asarray(proj['ps_wave_coefficients'], dtype=np.float32)
    if not np.all(weights[...,3][mask] == cb[0]):
        raise ValueError(f'{path}: measured wave Z differs from PS buffer')
    resources = []
    for k in range(4):
        rec = record(f'water_input_wave{k}')
        info = rec['water_input']
        resources.append(dict(sampler=info['sampler'], first_mip=info['most_detailed_mip'], mips=info['mip_levels'],
                              width=rec['source']['width'], height=rec['source']['height'],
                              mip0_sha256=hashlib.sha256(artifact_path(path, rec['raw']).read_bytes()).hexdigest()))
    shaded = load_stage(root, mode, spec, shot, (Stage.WATER_AFTER_LEFT, Stage.WATER_AFTER_RIGHT)[eye], eye).values[..., :3]
    return dict(mask=mask, sample=np.concatenate((a,b),axis=-1).reshape(*mask.shape,4,2),
                uv=np.concatenate((u,v),axis=-1).reshape(*mask.shape,4,2), weights=weights[...,:3], cb=cb,
                combined=combined, output=output[...,:3], shaded=shaded, resources=resources)


def operands(s, selection):
    samples = s['sample'][selection].astype(float)
    w = s['weights'][selection].astype(float)
    cb = s['cb'].astype(float)
    gain = np.concatenate((np.ones_like(w[..., :1]), w * cb[1:]), axis=-1)
    return samples, w, cb, gain


def gradient_pair(a,b,common,anchors):
    keys = ('uv','cx','cy','fx','fy','sample')
    changed = {k:np.any(a[k][common] != b[k][common],axis=-1) for k in keys}
    uv,cx,cy,fx,fy,sample = (changed[k] for k in keys)
    masks = dict(same_uv=~uv, same_uv_and_coarse=~(uv|cx|cy), same_uv_and_fine=~(uv|fx|fy),
                 same_uv_and_both=~(uv|cx|cy|fx|fy), different_uv_and_equal_gradients=uv&~(cx|cy|fx|fy))
    counts = {k:dict(total=m.sum(axis=0).tolist(),sample_changed=(m&sample).sum(axis=0).tolist()) for k,m in masks.items()}
    points = {}
    for name,(y,x) in anchors.items():
        points[name]=dict(x=x,y=y,common=bool(common[y,x]))
        if common[y,x]:
            points[name].update({k:[s[k][y,x].tolist() for s in (a,b)] for k in keys},
                                shaded_delta=(b['shaded'][y,x]-a['shaded'][y,x]).tolist())
    for i in range(2):
        where=np.flatnonzero(masks['same_uv_and_both'][:,i]&sample[:,i]);
        if where.size:
            y,x = map(int,np.unravel_index(np.flatnonzero(common)[where[0]],common.shape))
            points[f'equal_operands_t{i+8}']=dict(x=x,y=y,**{k:[s[k][y,x,i].tolist() for s in (a,b)] for k in keys})
    return dict(counts=counts,changed={k:m.sum(axis=0).tolist() for k,m in changed.items()}, anchors=points,
                identical_resource_and_sampler=[v==w for v,w in zip(a['resources'],b['resources'])])


def analyze_eye(manifest, case, strict, regions, eye, ref):
    label = f'{ref} vs single_pass'
    strict_group, = [g for g in strict['comparisons'] if g['name'] == f'{label}, eye {eye}']
    if strict_group['status'] not in ('fail','pass') or strict_group['errors']:
        return dict(eye=eye, status='ineligible', strict_errors=strict_group['errors'])
    regional, = [g for g in regions['comparisons'] if g['stage'] == ('water_after_left','water_after_right')[eye]
                 and g['name'] == label and g['eye'] == eye]
    pairs = [(p['a']['capture_id'], p['b']['capture_id']) for p in regional['pairs']]
    if (regional['errors'] or regional['status'] != 'measured' or len(pairs) != 18 or len(set(pairs)) != 18 or
            set(pairs) != {(p['capture_a'], p['capture_b']) for p in strict_group['pairs']}):
        raise ValueError(f'{label}: invalid matched pairs')
    shots = {}
    for group in case.modes:
        if group.mode in (ref, Mode.SINGLE_PASS):
            entries, errors = validated_samples(manifest.parent, group.mode, group.samples)
            if errors:
                raise ValueError(errors)
            for spec, shot in entries:
                shots[shot.capture_id] = load(manifest.parent, group.mode, spec, shot, eye)
    common = np.logical_and.reduce([s['mask'] for s in shots.values()])
    if not common.any():
        raise ValueError(f'{label}: no persistent common water')
    minimum = np.minimum.reduce([np.abs(shots[b]['output']-shots[a]['output']).mean(axis=-1) for a,b in pairs])
    y,x = np.unravel_index(np.argmax(np.where(common,minimum,-1)),common.shape)
    anchors = {'persistent_output_max':(int(y),int(x)), 'treated_214':((348,253),(291,221))[eye]}
    if 'cx' in next(iter(shots.values())):
        anchors['control_216'] = ((302,227),(347,253))[eye]
        return dict(eye=eye, status='measured',common=int(common.sum()),records={i:s['resources'] for i,s in shots.items()},
                    pairs=[dict(reference=ia,sps=ib,**gradient_pair(shots[ia],shots[ib],common,anchors)) for ia,ib in pairs])
    rows = []
    for ia,ib in pairs:
        a,b = shots[ia],shots[ib]
        sa,wa,ca,ga = operands(a,common)
        sb,wb,cb,gb = operands(b,common)
        sample_terms = (sb-sa)*ga[...,None]
        weight_terms = sb[:,1:] * ((wb-wa)*ca[1:])[...,None]
        coefficient_terms = sb[:,1:] * (wb*(cb[1:]-ca[1:]))[...,None]
        duv = b['uv'][common].astype(float)-a['uv'][common]
        measured = [s['combined'][common,:2].astype(float) for s in (a,b)]
        residual = [(s*g[...,None]).sum(axis=1)-v for s,g,v in zip((sa,sb),(ga,gb),measured)]
        point = {}
        for name,(py,px) in anchors.items():
            if not common[py,px]:
                point[name] = dict(x=px,y=py,common=False)
                continue
            j = np.searchsorted(np.flatnonzero(common), py*common.shape[1]+px)
            point[name] = dict(x=px,y=py,common=True, sample=[s[j].tolist() for s in (sa,sb)],
                uv=[s['uv'][py,px].tolist() for s in (a,b)], weights=[w[j].tolist() for w in (wa,wb)],
                cb=[c.tolist() for c in (ca,cb)], wave_xy=[v[j].tolist() for v in measured],
                mixed_xy=[s['combined'][py,px,2:].tolist() for s in (a,b)],
                sample_terms=sample_terms[j].tolist(),weight_terms=weight_terms[j].tolist(),
                coefficient_terms=coefficient_terms[j].tolist(),model_minus_measured=[r[j].tolist() for r in residual],
                shaded_delta=(b['shaded'][py,px]-a['shaded'][py,px]).tolist())
        uvdiff = np.any(duv != 0,axis=-1)
        sdiff = np.any(sb != sa,axis=-1)
        yy,xx = np.where(common)
        if common.shape[0] % 2 or common.shape[1] % 2:
            raise ValueError('wave footprint: expected even physical extent')
        quad = common[yy,xx^1] & common[yy^1,xx] & common[yy^1,xx^1]
        neighbors = np.stack([np.any(a['uv'][yy^dy,xx^dx] != b['uv'][yy^dy,xx^dx], axis=-1)
                              for dy,dx in ((0,1),(1,0),(1,1))]).any(axis=0)
        equal_center = sdiff & ~uvdiff
        neighbor_counts = dict(common_quad=int(quad.sum()), equal_center_in_quad=(equal_center & quad[:,None]).sum(axis=0).tolist(),
            equal_center_different_neighbor=(equal_center & neighbors & quad[:,None]).sum(axis=0).tolist(),
            equal_all_four=(equal_center & ~neighbors & quad[:,None]).sum(axis=0).tolist())
        rows.append(dict(reference=ia,sps=ib, anchors=point, aligned_visible_neighbors=neighbor_counts, changed_uv=uvdiff.sum(axis=0).tolist(),
            changed_sample=sdiff.sum(axis=0).tolist(), changed_sample_equal_uv=(sdiff & ~uvdiff).sum(axis=0).tolist(),
            changed_weight=(wb!=wa).sum(axis=0).tolist(), changed_combined=int(np.any(measured[0]!=measured[1],axis=-1).sum()),
            mean_abs_sample_terms=np.abs(sample_terms).mean(axis=(0,2)).tolist(),
            mean_abs_weight_terms=np.abs(weight_terms).mean(axis=(0,2)).tolist(),
            mean_abs_cb_terms=np.abs(coefficient_terms).mean(axis=(0,2)).tolist(),
            max_abs_model_residual=[float(np.abs(r).max()) for r in residual],
            shaded_mae=float(np.abs(b['shaded'][common]-a['shaded'][common]).mean())))
    return dict(eye=eye,status='measured',common=int(common.sum()),records={i:s['resources'] for i,s in shots.items()},pairs=rows)


def analyze(manifest, regional_path, case_name):
    cases = Manifest.from_wire(read_json(manifest)).cases
    case, = [c for c in cases if c.name == case_name]
    strict, regional = [read_json(p) for p in (manifest.parent/'comparison/report.json',regional_path)]
    if any(Path(r['manifest']).resolve()!=manifest.resolve() for r in (strict,regional)):
        raise ValueError('wave trace: mismatched reports')
    sc,rc = [next(c for c in r['cases'] if c['name']==case_name) for r in (strict,regional)]
    return [analyze_eye(manifest,case,sc,rc,e,ref) for e,ref in enumerate((Mode.LEFT_ONLY,Mode.RIGHT_ONLY))]


def producer_bindings(manifest, case_name, log_path):
    blocks = {}
    pending = {}
    for line in log_path.read_text().splitlines():
        if 'water probe: constant readback pass=109' in line:
            m = re.search(r'pass=109 eye=(\d+) invocation=(\d+) predicted_display_time_ns=(\d+).*?stage=(vs|hs|ds) slot=(\d+) pointer=(0x[0-9a-f]+) byte_width=(\d+) range_source=ctx1 first_constant=(\d+) num_constants=(\d+) values=(.*?) range_error=none', line)
            if m:
                tag = tuple(map(int,m.groups()[:3]))
                name = m[4]+m[5]
                packet = pending.setdefault(tag,{})
                if name in packet:
                    raise ValueError('duplicate constant before raw submission')
                rows = np.fromstring(m[10].replace('[',' ').replace(']',' '),sep=' ',dtype=np.float32)
                if rows.size==0 or rows.size%4:
                    raise ValueError('invalid buffer row readback')
                packet[name] = dict(pointer=int(m[6],16),width=int(m[7]),first=int(m[8]),count=int(m[9]),bits=rows.view(np.uint32).reshape(-1,4).tolist())
        elif 'water probe: raw draw pass=109' in line:
            m = re.search(r'pass=109 eye=(\d+) invocation=(\d+) predicted_display_time_ns=(\d+).*? log=(\d+)/',line)
            if not m:
                raise ValueError('malformed raw submission header')
            eye,inv,frame,index = map(int,m.groups())
            packet = pending.pop((eye,inv,frame),{})
            shaders = re.search(r'topology=35 .*?dsv=(0x[0-9a-f]+).*?vs=(0x[0-9a-f]+) hs=(0x[0-9a-f]+) ds=(0x[0-9a-f]+) gs=0x0 ps=(0x[0-9a-f]+)',line)
            if shaders and all(int(v,16) for v in shaders.groups()):
                key = (frame,eye,index)
                if key in blocks:
                    raise ValueError('duplicate raw submission')
                blocks[key] = dict(invocation=inv,shader_pointers=shaders.groups()[1:],buffers=packet)
    case,=[c for c in Manifest.from_wire(read_json(manifest)).cases if c.name==case_name]
    records={}
    for group in case.modes:
        for spec in group.samples:
            wire=read_json(manifest.parent/spec.metadata)
            for r in wire['stages']:
                if r['stage']!='water_input_wave1': continue
                info=r['water_input'];key=(r['frame'],r['eye'],info['draw_index']); block=blocks[key]
                if info['tess_index']!=7 or r['frame']!=wire['predicted_display_time_ns'] or int(block['shader_pointers'][-1],16)!=info['pixel_shader']:
                    raise ValueError('selected lake draw identity mismatch')
                if not {'ds1','ds4','hs3','vs0','vs1','vs2','vs3','vs4'}<=block['buffers'].keys():
                    raise ValueError('incomplete selected draw buffer evidence')
                selected, = [v for v in wire['stages'] if v['stage']=='water_input_static_reflection' and v['eye']==r['eye']]
                binding = selected['water_input']['projection'].get('ds_mvp_binding')
                if binding:
                    for stage, key in [('vs','original'),('ds','actual')]:
                        cb = block['buffers'][stage+'1']
                        if cb['pointer'] != binding[stage+'_pointer'] or sum(cb['bits'],[]) != binding[key]:
                            raise ValueError('producer: selected constant packet differs from independent readback')
                records.setdefault(wire['id'],{})[str(r['eye'])]=block
    strict=read_json(manifest.parent/'comparison/report.json'); sc,=[c for c in strict['cases'] if c['name']==case_name]
    groups=[]
    for group in sc['comparisons']:
        if group['status'] not in ('pass','fail') or group['errors']:
            groups.append(dict(name=group['name'],status='ineligible'));continue
        eye=group['name'][-1]; pairs=[]
        for row in group['pairs']:
            ia,ib=row['capture_a'],row['capture_b'];a,b=[records[i][eye]['buffers'] for i in (ia,ib)]
            def indices(k):
                v,w=(np.asarray(r[k]['bits'],dtype=np.uint32) for r in (a,b))
                if v.shape!=w.shape: return dict(shapes=[v.shape,w.shape])
                return np.argwhere(v!=w).tolist()
            pairs.append(dict(a=ia,b=ib,changed_row_components={k:indices(k) for k in sorted(a.keys()&b.keys())}))
        groups.append(dict(name=group['name'],status='measured',pairs=pairs))
    return dict(records=records,comparisons=groups)


def mvp_sources(manifest, case_name, producer):
    from tools.rendering.capture.validation import physical_water_mvp
    case, = (c for c in Manifest.from_wire(read_json(manifest)).cases if c.name == case_name)
    wires = [read_json(manifest.parent / sample.metadata) for group in case.modes for sample in group.samples]
    sources = {w['id']: [physical_water_mvp(w, eye) for eye in range(2)] for w in wires if w['mode'] == 'single_pass'}
    evidence = read_json(producer)
    results = []
    for group in evidence['comparisons']:
        if group['name'].startswith(('left_only vs single_pass', 'right_only vs single_pass')):
            eye = int(group['name'][-1])
            pairs = []
            for pair in group.get('pairs', []):
                iso, sps = pair['a'], pair['b']
                rows = evidence['records'][iso][str(eye)]['buffers']['ds1']['bits'][:4]
                reference = [v for row in rows for v in row]
                assert len(reference) == 16
                pairs.append(dict(isolated=iso, packed=sps, differing_words=[i for i in range(16) if reference[i] != sources[sps][eye][i]]))
            results.append(dict(eye=eye, status=group['status'], pairs=pairs))
    return results


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('manifest', type=Path)
    parser.add_argument('regions', type=Path)
    parser.add_argument('case')
    parser.add_argument('--producer-log',type=Path)
    parser.add_argument('--mvp-sources', type=Path)
    args = parser.parse_args()
    if args.mvp_sources:
        print(json.dumps(mvp_sources(args.manifest,args.case,args.mvp_sources),indent=2))
        raise SystemExit(0)
    if args.producer_log:
        print(json.dumps(producer_bindings(args.manifest,args.case,args.producer_log),indent=2))
        raise SystemExit(0)
    results = analyze(args.manifest,args.regions,args.case)
    print(json.dumps(results,indent=2,allow_nan=False))
    raise SystemExit(2 if any(r['status']!='measured' for r in results) else 0)
