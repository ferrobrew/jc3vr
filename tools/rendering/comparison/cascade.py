"""Compare a shore cascade ABBA experiment with the historical MP control split."""

import argparse
import json
from dataclasses import asdict, dataclass
from itertools import combinations, product
from pathlib import Path

import numpy as np

from tools.rendering.capture.validation import read_object, validate_resolve_inputs
from tools.rendering.comparison.images import ComparisonInputs, metadata_mismatches
from tools.rendering.comparison.stages import Source, artifact_path, decode_raw
from tools.rendering.protocol import CascadeScale, Mode


@dataclass(frozen=True)
class Shot:
    path: Path
    wire: dict
    level: CascadeScale

    def stage(self, name: str, eye: int) -> np.ndarray:
        record, = [r for r in self.wire['stages'] if r['stage'] == name and r['eye'] == eye]
        if record['error'] or record['frame'] != self.wire['predicted_display_time_ns']:
            raise ValueError(f'{self.path}: invalid {name} record')
        source = Source.from_wire(record['source'])
        raw = artifact_path(self.path, record['raw']).read_bytes()
        return decode_raw(raw, source)[1]

    @classmethod
    def load(cls, path: Path, treated: bool):
        wire = read_object(path)
        if wire['mode'] != Mode.MULTIPASS:
            raise ValueError(f'{path}: expected MP')
        if treated:
            level = CascadeScale(wire['cascade_scale'])
            validate_resolve_inputs(path.parent, wire['id'], wire, Mode.MULTIPASS, True, level)
        else:
            bits = [r['fragment_globals'][54]['bits'][0] for r in wire['constants']['lighting_records']
                    if r['frame'] == wire['predicted_display_time_ns'] and r['pass'] == 92]
            level, = [v for v in CascadeScale if bits == [v.bits] * 2]
        return cls(path, wire, level)


def shots(manifest: Path, treated: bool) -> list[Shot]:
    result = [Shot.load(manifest.parent / item['metadata'], treated)
              for case in read_object(manifest)['cases'] for item in case['modes']['multipass']]
    result.sort(key=lambda shot: shot.wire['frame_ordinal'])
    if (len({s.wire['predicted_display_time_ns'] for s in result}) != len(result) or
            any(s.wire['comparison_inputs'] != result[0].wire['comparison_inputs'] for s in result)):
        raise ValueError(f'{manifest}: duplicate frame or changing inputs')
    return result


@dataclass(frozen=True)
class MaskCounts:
    historical: int
    forced: int
    overlap: int
    same_direction: int
    historical_foreground_mismatch: int
    block_pixels: list[int]
    block_within_low: list[int]
    block_within_high: list[int]


def spatial(history: list[Shot], trial: list[Shot], eye: int) -> MaskCounts:
    old = [s.stage('post_resolve', eye) for s in history]
    new = [s.stage('post_resolve', eye) for s in trial]
    def significant(delta):
        return np.max(np.abs(delta), axis=-1) > 0.01
    pairs = [h - l for l, h in product(old[:2], old[2:])]
    within = [significant(a - b) for level in (old[:2], old[2:]) for a, b in combinations(level, 2)]
    historical = np.logical_and.reduce([significant(d) for d in pairs]) & ~np.logical_or.reduce(within)
    blocks = [new[i:i+4] for i in range(0, len(new), 4)]
    deltas = [((h1 - l1) + (h2 - l2)) / 2 for l1, h1, h2, l2 in blocks]
    low = [significant(l1 - l2) for l1, _, _, l2 in blocks]
    high = [significant(h1 - h2) for _, h1, h2, _ in blocks]
    forced = np.logical_and.reduce([significant(d) for d in deltas]) & ~np.logical_or.reduce(low + high)
    overlap = historical & forced
    agree = (np.sum(np.mean(pairs, axis=0) * np.mean(deltas, axis=0), axis=-1) > 0) & overlap
    foreground = [(s.stage('geometry_depth', eye)[..., 0] != 0) for s in history + trial]
    return MaskCounts(int(historical.sum()), int(forced.sum()), int(overlap.sum()), int(agree.sum()),
                      int((historical & ~np.logical_and.reduce(foreground)).sum()),
                      [int(significant(d).sum()) for d in deltas],
                      [int(d.sum()) for d in low], [int(d.sum()) for d in high])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('historical', type=Path)
    parser.add_argument('trial', type=Path)
    args = parser.parse_args()
    history, trial = shots(args.historical, False), shots(args.trial, True)
    lo, hi = CascadeScale.SHORE_LOW, CascadeScale.SHORE_HIGH
    if [s.level for s in history] != [lo]*2 + [hi]*4 or [s.level for s in trial] != [lo,hi,hi,lo]*3:
        raise ValueError('cascade: unexpected historical or trial sequence')
    inputs = [ComparisonInputs.from_wire(s.wire['comparison_inputs']) for s in (history[0], trial[0])]
    print(json.dumps({'historical_match_errors': metadata_mismatches(*inputs, 1e-5),
                      'eyes': [asdict(spatial(history, trial, eye)) for eye in range(2)]}, indent=2))


if __name__ == '__main__':
    main()
