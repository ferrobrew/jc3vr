#!/usr/bin/env python3
"""Review frozen ownership changes against existing obligations; output JSON only."""
import collections
import copy
import json
from pathlib import Path
import re
import sys

import export_fixture as e

ROOT = Path(__file__).resolve().parent
ROUTES = {
    'payload/src/ui/game.rs': {
        'swimming_controls': ('Swimming', 'Controls'),
        'locomotion_controls': ('Locomotion', 'Controls'),
        'relaxed_strafe_controls': ('Locomotion', 'Controls'),
        'driving_controls': ('Driving', 'Controls'),
        'parachute_controls': ('Parachute', 'Controls'),
        'wingsuit_controls': ('Wingsuit', 'Controls'),
        'egui_debug_game': ('Engine', 'Controls'),
        'sim_rate_controls': ('Engine', 'Controls'),
    },
    'payload/src/ui/diagnostics.rs': {
        'render_capture_controls': ('RenderCapture', 'Controls'),
        'diagnostics_controls': ('RenderDiagnostics', 'Controls'),
        'show_render_block_type_registry': ('RenderDiagnostics', 'Controls'),
        'show_unregistered_render_block_types': ('RenderDiagnostics', 'Controls'),
    },
    'payload/src/ui/render/far_field.rs': {'section': ('Graphics', 'Controls')},
    'payload/src/ui/controllers/physical_hands.rs': {
        'vault_settings': ('CharactersDoors', 'Controls'),
        'characters_settings': ('CharactersDoors', 'Controls'),
        'doors_settings': ('CharactersDoors', 'Controls'),
        'sounds_settings': ('Feedback', 'Controls'),
    },
    'payload/src/ui/controllers/loadout.rs': {
        'controls': ('Sandbox', 'Controls'), 'pick': ('Sandbox', 'Controls'),
    },
}
CFG_HELPERS = {'swimming_controls', 'locomotion_controls', 'driving_controls',
               'parachute_controls', 'wingsuit_controls'}


def normalized(text):
    return re.sub(r'\s+', ' ', text).strip()


def build():
    mapping = json.loads((ROOT / 'mapping.json').read_text())
    assert mapping['scanner'] == e.CURRENT_VERSION
    before = copy.deepcopy(mapping)
    by_path = collections.defaultdict(list)
    for row in mapping['assignments']:
        by_path[row['path']].append(row)
    changes = []
    drifts = []
    roots = []
    proof = {}
    for path, rows in sorted(by_path.items()):
        text = Path(path).read_text()
        records = e.scanner.scan(Path('.'), {'path': path, 'surface': 'Debug', 'candidate_destinations': []})
        indexed = collections.defaultdict(list)
        for record in records:
            if record['kind'] in e.ACTIONABLE:
                indexed[(record['kind'], record['normalized'])].append(record)
        for row in rows:
            candidates = indexed[(row['kind'], row['semantic']['call'])]
            witnesses = [(r, e.witness(text, r)) for r in candidates]
            exact = [(r, w) for r, w in witnesses if w == row['semantic']]
            selected = exact if len(exact) == row['expected_matches'] else witnesses
            assert len(selected) == row['expected_matches'], row['old_id']
            record, semantic = selected[row['occurrence'] - 1]
            original = copy.deepcopy(row)
            if semantic != row['semantic']:
                assert path in ('payload/src/ui/game.rs', 'payload/src/ui/diagnostics.rs'), row['old_id']
                assert normalized(semantic['effect']) == normalized(row['semantic']['effect']), row['old_id']
                if path.endswith('/game.rs') and record['function'] in CFG_HELPERS:
                    assert row['semantic']['bindings'] == ['let mut cfg = config : : edit ( ) ;']
                    assert semantic['bindings'] == []
                    assert f"fn {record['function']}(ui: &mut egui::Ui, cfg: &mut Config)" in text
                    call = f"{record['function']}(ui, &mut config::edit());"
                    assert call in text
                    root = {'path': path, 'function': record['function'],
                            'signature': f"fn {record['function']}(ui: &mut egui::Ui, cfg: &mut Config)",
                            'call': call, 'root': 'config::Config',
                            'targets': re.findall(r'cfg (?:\. [A-Za-z_][A-Za-z0-9_]* ?)+', row['semantic']['call'])}
                    row['config_root_review'] = root
                    roots.append(row['old_id'])
                else:
                    assert (path.endswith('/game.rs') and record['function'] == 'egui_debug_game') or (
                        path.endswith('/diagnostics.rs') and record['function'] == 'diagnostics_controls')
                row['semantic'] = semantic
                row['review'] += ' Reviewed frozen editor extraction: exact primitive call and whitespace-normalized attached effect retained; local source bindings now reflect the extracted body. Config helper parameters are explicitly bound to the same Config root by the recorded call site.'
                drifts.append(row['old_id'])
            route = ROUTES.get(path, {}).get(record['function'])
            if route:
                owners = [{'feature': route[0], 'section': route[1]}]
                if row['owners'] != owners:
                    row['owners'] = owners
                    row['route_evidence'] = [f"{path}::{record['function']}; production feature match and direct editor call reviewed in frozen finish-render-ui-routes / finish-controller-ui-routes."]
                    row['review'] += ' Reviewed owner/section correction: direct feature route now owns the retained editor; no setting target or command sink is removed.'
            if row != original:
                changes.append({'old_id': row['old_id'], 'old_owners': original['owners'],
                                'new_owners': row['owners'], 'current_function': record['function'],
                                'witness_changed': original['semantic'] != row['semantic']})
            proof[row['old_id']] = (record, semantic)
    assert len(mapping['assignments']) == 1405
    assert len(drifts) == 24
    assert len(roots) == 21
    mapping['owner_route_review'] = {
        'scanner': e.CURRENT_VERSION,
        'reason': 'Reviewed frozen direct feature routes after controller/render ownership fixes. Existing historical IDs select unchanged primitive calls. Exact attached effects are retained after whitespace normalization; Config helper parameter roots are recorded explicitly. Function names remain evidence, not target identity. New navigation/drawer/overview scaffolding creates no historical obligations and removes no mapped widget targets.',
        'changed_assignments': len(changes), 'witness_changed_assignments': len(drifts),
        'config_root_assignments': len(roots), 'changes': changes,
        'source_sha256': {p: e.hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in ROUTES},
        'status_route_review': {
            'physical_hands::characters_doors_readout': 'CharactersDoors/Status',
            'physical_hands::impact_sounds_readout': 'Feedback/Status',
            'loadout::live_readout': 'Inventory/Status', 'loadout::sandbox_readout': 'Sandbox/Status',
            'vehicles::feature_readout::Driving + driving_view_readout': 'Driving/Status', 'vehicles::physical_readout': 'VehicleControls/Status',
            'vehicles::mounted_readout': 'Mounted/Status', 'vehicles::arms_readout': 'Wingsuit/Status',
            'vehicles::handles_readout': 'Parachute/Status',
            'game::swimming_readout': 'Swimming/Status', 'game::locomotion_readout': 'Locomotion/Status',
            'game::parachute_readout': 'Parachute/Status', 'game::wingsuit_readout': 'Wingsuit/Status',
            'game::game_readout': 'Engine/Status', 'game::sim_rate_readout': 'Engine/Status',
            'diagnostics::render_capture_readout': 'RenderCapture/Status',
            'diagnostics::logging_readout': 'RenderDiagnostics/Status',
            'diagnostics::camera_visualization': 'RenderDiagnostics/Visualizations',
            'diagnostics::show_camera_snapshot': 'RenderDiagnostics/Visualizations',
            'far_field::readout': 'Graphics/Status',
        },
    }
    assert before['baseline_commit'] == mapping['baseline_commit']
    assert before['consolidations'] == mapping['consolidations']
    return mapping


if __name__ == '__main__':
    result = build()
    if len(sys.argv) > 1 and sys.argv[1] == 'report':
        print(json.dumps(result['owner_route_review'], indent=2))
    else:
        print(json.dumps(result, ensure_ascii=False, separators=(',', ':')))
