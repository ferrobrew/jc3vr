#!/usr/bin/env python3
"""Propose only v1-to-v2 current evidence changes; never rewrite historical obligations.

The existing path, kind, and exact v1 witness select the reviewed source record. No
candidate destination, function name, or current inventory supplies an obligation.
JSON output can be captured separately, inspected, then copied to mapping.json.
"""
import collections
import copy
import json
from pathlib import Path
import sys

import export_fixture as exporter

ROOT = Path(__file__).resolve().parent


def upgrade():
    mapping = json.loads((ROOT / 'mapping.json').read_text())
    assert mapping['scanner'] == exporter.VERSION
    by_path = collections.defaultdict(list)
    for row in mapping['assignments']:
        by_path[row['path']].append(row)
    changed = []
    for path, rows in sorted(by_path.items()):
        text = Path(path).read_text()
        records = exporter.scanner.scan(Path('.'), {
            'path': path, 'surface': 'Debug', 'candidate_destinations': []})
        indexed = collections.defaultdict(list)
        for record in records:
            if record['kind'] not in exporter.ACTIONABLE:
                continue
            semantic = exporter.witness(text, record, exporter.VERSION)
            indexed[(record['kind'], json.dumps(semantic, sort_keys=True))].append(record)
        for row in rows:
            key = (row['kind'], json.dumps(row['semantic'], sort_keys=True))
            matches = indexed[key]
            assert len(matches) == row['expected_matches'], row['old_id']
            record = matches[row['occurrence'] - 1]
            semantic = exporter.witness(text, record, exporter.CURRENT_VERSION)
            if semantic == row['semantic']:
                continue
            changed.append({'old_id': row['old_id'], 'path': path,
                            'before': copy.deepcopy(row['semantic']), 'after': semantic})
            row['semantic'] = semantic
            row['review'] += ' Current witness v2 review: delimiter-aware suffix capture retains nested argument blocks and the later attached handler or setter body; the target, owner, and occurrence are unchanged.'
    mapping['scanner'] = exporter.CURRENT_VERSION
    mapping['current_witness_review'] = {
        'previous_scanner': exporter.VERSION,
        'changed_assignments': len(changed),
        'old_ids': [row['old_id'] for row in changed],
        'reason': 'The v1 suffix scanner stopped at the first block, including blocks nested in builder arguments. V2 continues through nested arguments to the attached handler, retaining a nested closure when no later handler follows. Only current semantic evidence for the listed IDs changes; historical obligations and all path, kind, owner, section, occurrence, and consolidation assignments remain unchanged.'}
    return mapping, changed


if __name__ == '__main__':
    mapping, changed = upgrade()
    if len(sys.argv) > 1 and sys.argv[1] == 'report':
        print(json.dumps(changed, ensure_ascii=False, indent=2))
    else:
        print(json.dumps(mapping, ensure_ascii=False, separators=(',', ':')))
