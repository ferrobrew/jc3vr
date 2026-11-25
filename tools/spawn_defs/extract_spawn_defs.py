#!/usr/bin/env python3
"""Build the payload's spawn definition table from the game's spawn definition files.

The spawn system selects what to spawn by matching a requested tag set against the tag strings of
its definitions, which ship as RTPC property containers (`settings/spawn_*_defs.bin`, referenced by
`settings/spawn_budget_pools.bin`). This script reads those files after they have been extracted
and converted to XML with the Gibbed tools (see the reverse-engineering skill for the extraction
workflow) and writes one compact JSON table the payload embeds, so the spawner's picker offers the
game's own definitions by name, category, and tag set. Each entry also carries the `hashlittle` hash
of its entity's name (`entity_hash`), which is the name hash a spawned weapon carries and the key of
its display name in the game's localisation store.

Usage:
    extract_spawn_defs.py <dir with spawn_*_defs.xml> [output.json]

The output defaults to `payload/src/director/spawn_defs.json` relative to the repository root.
"""

import json
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "sights"))
from archives import hashlittle  # noqa: E402

# The definition files whose entries are worth spawning by hand. The road decorations, trains,
# effects, and encounters are placement-system objects that do not spawn meaningfully in isolation.
FILES = [
    "spawn_vehicle_defs.xml",
    "spawn_vehicle_premium_defs.xml",
    "spawn_aircraft_defs.xml",
    "spawn_aircraft_premium_defs.xml",
    "spawn_boat_defs.xml",
    "spawn_combatant_defs.xml",
    "spawn_civilian_defs.xml",
    "spawn_animals_defs.xml",
    "spawn_prop_defs.xml",
    "spawn_device_defs.xml",
    "spawn_weapon_defs.xml",
]

# The display-name value the definitions carry under a hashed key.
DISPLAY_NAME_ID = "F71C2A21"


def entries(path: Path):
    tree = ET.parse(path)
    for obj in tree.getroot().iter("object"):
        values = {}
        for value in obj.findall("value"):
            key = value.get("name") or value.get("id")
            values[key] = value.text or ""
        tags = values.get("tags", "").strip()
        entity = values.get("entity", "")
        if not tags or not entity:
            continue
        # The tag strings mix comma and space separators across files; normalize to commas so the
        # payload can hand them straight to the spawn system, which accepts either.
        tags = ",".join(t for t in tags.replace(",", " ").split() if t)
        name = values.get(DISPLAY_NAME_ID) or obj.get("name") or Path(entity).stem
        yield {
            "name": name,
            "category": values.get("debug", ""),
            "tags": tags,
            "pool": path.stem.removeprefix("spawn_").removesuffix("_defs"),
            "entity_hash": hashlittle(Path(entity.replace("\\", "/")).stem),
        }


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    source = Path(argv[1])
    repo = Path(__file__).resolve().parents[2]
    output = Path(argv[2]) if len(argv) > 2 else repo / "payload/src/director/spawn_defs.json"
    table = []
    for name in FILES:
        path = source / name
        if not path.exists():
            print(f"missing {path}, skipping", file=sys.stderr)
            continue
        table.extend(entries(path))
    table.sort(key=lambda e: (e["pool"], e["category"], e["name"]))
    output.write_text(json.dumps(table, indent=1) + "\n")
    print(f"wrote {len(table)} definitions to {output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
