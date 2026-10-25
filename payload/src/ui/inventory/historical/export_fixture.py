#!/usr/bin/env python3
"""One-time transformation of the inspected audit evidence, not a test-time baseline builder.

Run from the repository root with `baseline` or `mapping`; JSON goes to stdout.
The ignored audit directory is required only to reproduce the initial fixture export.
"""
import bisect
import collections
import functools
import hashlib
import json
from pathlib import Path
import sys

AUDIT = Path('target/gameplay-review-20261024/ui-parity')
sys.path.insert(0, str(AUDIT))
import scan_inventory as scanner

ACTIONABLE = {'Setting', 'Command', 'Selection', 'TextInput', 'PauseItem'}
COMMIT = '6eeb8e17e62f19f8b63fc649a6ad4be9923f97f8'
VERSION = 'jc3vr-actionable-witness-v1'
CURRENT_VERSION = 'jc3vr-actionable-witness-v2'


@functools.cache
def source_tokens(text):
    tokens = scanner.tokenize(text)
    newlines = [i for i, char in enumerate(text) if char == '\n']
    lines = collections.defaultdict(list)
    for i, token in enumerate(tokens):
        lines[bisect.bisect_left(newlines, token[1]) + 1].append(i)
    return tokens, lines


def witness(text, record, version=CURRENT_VERSION):
    return cached_witness(text, record['line'], record['primitive'], record['kind'], record['normalized'], version)


@functools.cache
def cached_witness(text, line, primitive, kind, normalized, version):
    tokens, lines = source_tokens(text)
    starts = [i for i in lines[line] if tokens[i][0] == primitive]
    def call_end(i):
        return scanner.matching_end(tokens, i + 2 if tokens[i + 1][0] == '!' else i + 1)
    index = next(i for i in starts if scanner.primitive_kind(tokens, i) == kind and call_end(i) is not None and ' '.join(t[0] for t in tokens[i:call_end(i) + 1]) == normalized)
    end = call_end(index)
    body = 0
    for i in range(index):
        if tokens[i][0] == 'fn' and i + 2 < len(tokens) and tokens[i + 2][0] == '(':
            start = next((j for j in range(i + 2, len(tokens)) if tokens[j][0] == '{'), None)
            stop = scanner.matching_end(tokens, start) if start is not None else None
            if stop is not None and start < index < stop:
                body = start
    # Preserve the attached click block, setter closure, or pause row model.
    tail = []
    delimiters = []
    effect_end = None
    cursor = end + 1
    while cursor < len(tokens):
        value = tokens[cursor][0]
        if value in (';', '}') and not delimiters:
            break
        if value in ('(', '['):
            delimiters.append(')' if value == '(' else ']')
        elif value in (')', ']') and delimiters and delimiters[-1] == value:
            delimiters.pop()
        elif value == '{':
            stop = scanner.matching_end(tokens, cursor)
            if stop is None:
                break
            effect_end = stop
            # A builder argument's block must not hide a later attached click handler.
            if version == VERSION or kind == 'PauseItem' or not delimiters:
                break
            cursor = stop
        cursor += 1
    if effect_end is not None:
        tail = [t[0] for t in tokens[end + 1:effect_end + 1]]
    names = {t[0] for t in tokens[index:end + 1]} | set(tail)
    bindings = []
    # Follow local aliases recursively. Function names and source lines are not identity.
    for _ in range(8):
        previous = len(names)
        for i in range(body + 1, index):
            if tokens[i][0] != 'let':
                continue
            j = i + 1
            if tokens[j][0] == 'mut':
                j += 1
            if tokens[j][0] not in names:
                continue
            stop = next((k for k in range(j, index) if tokens[k][0] == ';'), None)
            if stop is None:
                continue
            binding = ' '.join(t[0] for t in tokens[i:stop + 1])
            if binding not in bindings:
                bindings.append(binding)
            names.update(t[0] for t in tokens[i:stop + 1])
        if len(names) == previous:
            break
    return {'call': normalized, 'bindings': sorted(bindings), 'effect': ' '.join(tail)}


def load():
    baseline = json.loads((AUDIT / 'baseline-controls.json').read_text())
    current = json.loads((AUDIT / 'current-controls.json').read_text())
    mappings = json.loads((AUDIT / 'resolved-mapping.json').read_text())
    return baseline, current, mappings


def export_baseline(baseline):
    controls = []
    for record in baseline['controls']:
        if record['kind'] not in ACTIONABLE:
            continue
        text = (AUDIT / 'baseline' / record['path']).read_text()
        controls.append({'id': record['id'], 'path': record['path'], 'line': record['line'],
                         'kind': record['kind'], 'surface': record['surface'],
                         'arguments': record['arguments'], 'semantic': witness(text, record, VERSION)})
    hashes = {entry['path']: hashlib.sha256((AUDIT / 'baseline' / entry['path']).read_bytes()).hexdigest()
              for entry in baseline['sources']}
    return {'schema': 1, 'commit': COMMIT, 'scanner': VERSION,
            'lexical_scanner_sha256': hashlib.sha256((AUDIT / 'baseline/payload/src/ui/inventory.rs').read_bytes()).hexdigest(),
            'python_port_sha256': hashlib.sha256((AUDIT / 'scan_inventory.py').read_bytes()).hexdigest(),
            'source_manifest_sha256': hashlib.sha256((AUDIT / 'baseline/payload/src/ui/inventory/sources.rs').read_bytes()).hexdigest(),
            'verification': 'All 64 baseline blobs match git show at the recorded commit; all 3058 lexical records rescan identically. Python port inspected against Rust scanner and four existing fixture shapes checked in Python. Rust execution remains pending.',
            'source_sha256': hashes, 'controls': controls}


def export_mapping(baseline, current, mappings):
    by_id = {r['id']: r for r in current['controls']}
    old_by_id = {r['id']: r for r in baseline['controls']}
    mapped = []
    for row in mappings['controls']:
        if row['kind'] not in ACTIONABLE:
            continue
        assert len(row['current_controls']) == 1
        control = by_id[row['current_controls'][0]['id']]
        semantic = witness((AUDIT / 'current' / control['path']).read_text(), control)
        peers = [r for r in current['controls'] if r['path'] == control['path'] and r['kind'] == control['kind'] and r['normalized'] == control['normalized']]
        peers = [r for r in peers if witness((AUDIT / 'current' / r['path']).read_text(), r) == semantic]
        ordinal = next(i for i, r in enumerate(peers, 1) if r['id'] == control['id'])
        old = old_by_id[row['baseline_id']]
        old_semantic = witness((AUDIT / 'baseline' / old['path']).read_text(), old, VERSION)
        mapped.append({'old_id': row['baseline_id'], 'path': control['path'], 'kind': control['kind'],
                       'semantic': semantic, 'occurrence': ordinal, 'expected_matches': len(peers),
                       'owners': [{'feature': r['feature'], 'section': r['section']} for r in row['actual_routes']],
                       'route_evidence': [r['evidence'] for r in row['actual_routes']],
                       'review': row['note'] or ('Exact semantic witness retained.' if old_semantic == semantic else 'Retained target and effect after editor extraction; local context differs.'),
                       'consolidation': None})
    groups = collections.defaultdict(list)
    for row in mapped:
        groups[(row['path'], json.dumps(row['semantic'], sort_keys=True), row['occurrence'])].append(row)
    consolidations = []
    for rows in groups.values():
        if len(rows) < 2:
            continue
        assert len(rows) == 2 and all('camera.rs' in r['path'] for r in rows), rows
        key = 'duplicate-turn-' + str(len(consolidations) + 1)
        for row in rows:
            row['consolidation'] = key
        consolidations.append({'id': key, 'old_ids': [r['old_id'] for r in rows],
                               'reason': 'The Controllers duplicate and Camera editor control the same headpose.vr_turn field or mode choice. The Camera editor retains the target. Smooth and snap ranges cover the old ranges; scales are mode-gated under Locomotion Controls advanced tuning.'})
    assert len(mapped) == 1405 and len(consolidations) == 4
    return {'schema': 1, 'baseline_commit': COMMIT, 'scanner': CURRENT_VERSION,
            'review_scope': 'Source-reviewed audit assignments, not route execution. Owner and section changes require review. Function names are evidence only and are excluded from target identity.',
            'consolidations': consolidations, 'assignments': mapped}


if __name__ == '__main__':
    baseline, current, mappings = load()
    result = export_baseline(baseline) if sys.argv[1] == 'baseline' else export_mapping(baseline, current, mappings)
    print(json.dumps(result, ensure_ascii=False, separators=(',', ':')))
