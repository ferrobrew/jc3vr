import argparse
import json
from pathlib import Path

import numpy as np


def normalize(v):
    return v / np.linalg.norm(v, axis=-1, keepdims=True)


def candidates(record, packed, points, eye):
    inputs = record['impostor']
    cb = {(r['stage'], r['slot']): np.asarray(r['bits'], '<u4').view('<f4').reshape(-1, 4)
          for r in inputs['cb'] if r['bits'] is not None}
    globals_, block, types = (cb['vs', i] for i in (0, 1, 3))
    cards = np.asarray(inputs['cards'], '<u4')
    triangles = np.asarray(inputs['indices']).reshape(-1, 3)
    if not np.all(triangles[:, :1] // 4 == triangles // 4):
        raise ValueError('impostor: mixed-card triangle')
    kind = cards[:, 2] >> 16
    xz = np.stack([cards[:, 1] & 65535, cards[:, 1] >> 16], axis=1).astype('f') * block[0, :2] / 32767 + block[0, 2:]
    base = np.stack([xz[:, 0], cards[:, 0].copy().view('<f4'), xz[:, 1]], axis=1)
    shape = types[kind*8+6]
    delta = base - globals_[9, :3]
    fade = np.minimum(1-np.clip(np.max(np.abs(delta)*[1, 0.0625, 1], axis=1)*block[1, 3]-block[1, 2], 0, 1),
                      np.clip(np.linalg.norm(delta, axis=1)*block[1, 1]-block[1, 0], 0, 1))
    center = base.copy()
    center[:, 1] += shape[:, 1]
    direction = normalize(globals_[9, :3] - center)
    side = normalize(np.stack([direction[:, 2], direction[:, 2]*0, -direction[:, 0]], axis=1))
    up = np.cross(direction, side)
    corners = 500 * types[kind[:, None]*8+np.arange(4), :2]
    vertices = (base[:, None, :] + corners[:, :, :1]*side[:, None, :] + corners[:, :, 1:]*up[:, None, :]
                + (shape[:, :1]*0.5*direction)[:, None, :])
    clip = vertices @ globals_[:3] + globals_[3]
    card_clip = (vertices - (shape[:, :1]*0.5*direction)[:, None, :]) @ globals_[:3] + globals_[3]
    offset = (clip[:, :, :2]/clip[:, :, 3:] - card_clip[:, :, :2]/card_clip[:, :, 3:]) / (2*block[3, :2])
    raster = clip.copy()
    raster[:, :, 2] = np.where(fade[:, None] <= 0.01, -clip[:, :, 3], clip[:, :, 2])
    if packed:
        raster = raster @ cb['vs', 13][10+4*eye:14+4*eye].T
    screen = (raster[:, :, :2]/raster[:, :, 3:] * [0.5,-0.5] + 0.5)*[320,360]
    tri = screen.reshape(-1,2)[triangles]
    cross = lambda a,b: a[...,0]*b[...,1]-a[...,1]*b[...,0]
    area = cross(tri[:,1]-tri[:,0],tri[:,2]-tri[:,0]);valid = np.abs(area)>1e-6
    out = {}
    for x,y in points:
        xy = np.array([x+.5,y+.5]); weights = np.stack([cross(tri[:,(i+1)%3]-xy,tri[:,(i+2)%3]-xy) for i in range(3)],axis=1) / np.where(valid,area,1)[:,None]
        hits = []
        for i in np.flatnonzero(valid & (weights.min(axis=1) >= -1e-5)):
            card = triangles[i,0]//4; idx = triangles[i]; r = raster.reshape(-1,4)[idx]
            if np.min(r[:,3]) <= 0 or np.max(r[:,2]) < 0: continue
            attr = (weights[i] / r[:,3]) @ clip.reshape(-1,4)[idx]
            q = (attr[:2]/attr[3]) / (2*cb['ps',0][8,2:]); h = float((q @ [.4679439961910248 * (0.5 if record.get("group_fade") is True else 1.0),-.7036479711532593])%1)
            persp = weights[i] / r[:, 3]; persp /= persp.sum()
            shift = persp @ offset.reshape(-1, 2)[idx]
            hits.append(dict(atlas_offset_xy=shift.tolist(), card=int(card), tri=int(i), fade=float(fade[card]), hash=h, pass_fade=h>=1-float(fade[card]), local_hash_xy=q.tolist(), raster_depth=float(weights[i]@(r[:,2]/r[:,3]))))
        out[f'{x},{y}'] = hits
    return out


def analyze(path, index_count):
    j = json.loads(path.read_text())
    traces = [s for s in j['stages'] if s['stage']=='interval_trace']
    for stage in traces:
        if stage['error'] or stage['frame'] != j['predicted_display_time_ns']:
            raise ValueError(f'{path}: invalid interval record')
        t = stage['velocity']
        d, = [d for d in t['draws'] if d.get('impostor') and d['args'][0] == index_count]
        inputs = d['impostor']
        count = len(t['points'])
        dual = t['packed'] and len(d['before']) == 2*count
        for eye in range(2) if dual else [stage['eye']]:
            start = eye*count if dual else 0
            yield {'id': j['id'], 'eye': eye, 'indices': index_count, 'raster': inputs.get('raster'), 'group_fade': d.get('group_fade'), 'card_atlas': d.get('card_atlas'),
                   'before': d['before'][start:start+count], 'after': d['after'][start:start+count],
                   'candidates': candidates(d, t['packed'], t['points'], eye)}


if __name__ == '__main__':
    parser=argparse.ArgumentParser(); parser.add_argument('captures', type=Path)
    parser.add_argument('--indices', type=int, choices=(3300, 1392, 144, 4872, 5946, 6534, 7542), default=3300); args=parser.parse_args()
    print(json.dumps([v for path in sorted(args.captures.glob('*.capture.json')) for v in analyze(path, args.indices)],indent=2,allow_nan=False))
