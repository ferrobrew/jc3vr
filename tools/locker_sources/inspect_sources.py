"""Inspect selected shipped RTPC sources with archive and property provenance.

The file-list search is intentionally separate from extraction. Only explicitly supplied paths are
read; entity archives expose their embedded RTPC members, with standalone patch replacements taking
precedence. Output is JSON for comparison with native property readers.
"""

import argparse
import json
import logging
import os
from pathlib import Path
import re
import sys

SIGHTS = Path(__file__).resolve().parents[1] / "sights"
sys.path.insert(0, str(SIGHTS))

from archives import GameArchives, hashlittle, read_entity
from rtpc import parse
from model_collection import parse_collection

log = logging.getLogger(__name__)

PROPERTY_NAMES = (
    "_class", "name", "name_hash", "world", "model", "model_file", "model_path",
    "model_xml", "class", "object_id", "_id", "_object_id", "script", "enabled",
    "spawn_tag", "weapon", "weapon_name", "weapon_type", "weapon_class", "ammo",
    "ammo_count", "weapon_spawn", "weapon_spawn_tag", "infinite_pickups", "infinite_ammo",
    "respawn", "respawn_time", "model_skeleton", "parent", "transform", "position",
    "entity", "entity_path", "filename", "file", "type", "intensity", "radius",
)
KNOWN = {hashlittle(name): name for name in PROPERTY_NAMES}
KNOWN.update({
    0xF9DCF6AB: "model_reference",
    0xF16ECA7D: "collision_model_reference",
    0x34BEEC18: "spawn_tags",
    0x3C098CC1: "infinite_pickups",
    0x9A7D17A0: "extra_pickup_ammo",
    0xCFA0E69F: "weapon_should_be_invisible",
    0xB52155E4: "weapon_is_in_rack",
    0x4CF3E5EB: "rack_uses_ordinary_pickup",
    0x54697E8F: "tags",
})


def file_names(root):
    """Normalize and deduplicate paths from the shipped resolved file lists."""
    return sorted({line.strip().replace("\\", "/")
                   for path in root.rglob("*.filelist")
                   for line in path.read_text().splitlines()
                   if line.strip() and not line.startswith(";")})


def walk(node, ancestry=()):
    """Yield every node, including children without a named native class."""
    label = node.get("name") or node.get("_class") or f"{node.name_hash:08x}"
    ancestry = (*ancestry, str(label))
    properties = {KNOWN.get(key, f"0x{key:08x}"): value.hex() if isinstance(value, bytes) else value
                  for key, value in node.properties.items()}
    yield {"node": "/".join(ancestry), "node_hash": f"0x{node.name_hash:08x}",
           "class": node.get("_class"), "properties": properties}
    for child in node.children:
        yield from walk(child, ancestry)


def inspect(archives, path):
    """Read a selected path, preserving the copy and effective member provenance."""
    data = archives.read(path)
    location = archives.locate(path)
    if data is None:
        raise FileNotFoundError(f"locker sources: {path}: not found in the base or patch archives")
    result = {"path": path, "archive": location.label, "size": len(data),
              "magic": data[:4].hex(), "members": []}
    if data.startswith(b"AAF\0") or data[4:8] == b"SARC":
        embedded = read_entity(data)
        result["embedded_files"] = sorted(embedded)
        candidates = list(embedded.items())
    else:
        candidates = [(path, data)]
    result["model_collections"] = []
    for member_path, member_data in candidates:
        replacement = archives.read(member_path) if member_path != path else None
        member_source = location.label if member_path == path else f"{location.label}:{path}"
        if replacement is not None:
            member_data = replacement
            member_source = archives.locate(member_path).label
        if member_data.startswith(b"MDI\0"):
            instances = parse_collection(member_data)
            for instance in instances:
                instance["model_hash"] = f"0x{hashlittle(instance['model']):08x}"
            result["model_collections"].append({
                "path": member_path, "archive": member_source,
                "instance_count": len(instances), "instances": instances,
            })
        if not member_data.startswith(b"RTPC"):
            continue
        nodes = list(walk(parse(member_data)))
        names_by_hash = {hashlittle(name): name for name in result.get("embedded_files", [])}
        for node in nodes:
            node["resolved_references"] = {
                key: names_by_hash[value & 0xFFFFFFFF]
                for key, value in node["properties"].items()
                if isinstance(value, int) and (value & 0xFFFFFFFF) in names_by_hash
            }
        result["members"].append({"path": member_path, "archive": member_source, "nodes": nodes})
    if not result["members"]:
        result["strings"] = [match.decode("ascii") for match in re.findall(rb"[ -~]{6,}", data)]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*")
    parser.add_argument("--game-dir", type=Path, default=os.environ.get("JC3_GAME_DIR"))
    parser.add_argument("--file-lists", type=Path)
    parser.add_argument("--search", help="Search normalized file-list names without extracting.")
    parser.add_argument("--match", help="Only emit nodes whose complete JSON matches this regex.")
    args = parser.parse_args()
    if args.search:
        if args.file_lists is None:
            tools = os.environ.get("JC3_GIBBED_TOOLS")
            if not tools:
                parser.error("--file-lists or JC3_GIBBED_TOOLS is required for --search")
            args.file_lists = Path(tools) / "bin/projects/Just Cause 3/files"
        pattern = re.compile(args.search, re.IGNORECASE)
        print(json.dumps([name for name in file_names(args.file_lists) if pattern.search(name)], indent=2))
        return
    if not args.game_dir or not args.paths:
        parser.error("selected paths and --game-dir or JC3_GAME_DIR are required")
    archives = GameArchives(args.game_dir)
    results = [inspect(archives, path.replace("\\", "/")) for path in args.paths]
    if args.match:
        pattern = re.compile(args.match, re.IGNORECASE)
        for result in results:
            for collection in result["model_collections"]:
                collection["instances"] = [instance for instance in collection["instances"]
                                           if pattern.search(json.dumps(instance))]
            for member in result["members"]:
                member["nodes"] = [node for node in member["nodes"] if pattern.search(json.dumps(node))]
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(levelname)s: %(message)s")
    main()
