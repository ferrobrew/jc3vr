#!/usr/bin/env python3
"""Derive the player weapons' sight lines from the game's models and write a sight override table.

For every player weapon entity (``editor/entities/jc_weapons/01_one_handed`` and ``02_two_handed``),
this reads the entity archive from the game's archives, takes the patch archive's standalone
``.epe`` over the embedded one where one exists, loads the fire point and the closest-LOD models, and
derives the sight line in the fire point's frame (see ``sightline.py``). The weapon list comes from
the Gibbed tools' file lists, which map archive name hashes back to paths.

Usage::

    extract_sights.py [--game DIR] [--gibbed-tools DIR] [--output FILE] [--dump DIR] [--plots DIR]

``--game`` defaults to ``$JC3_GAME_DIR``, then to the default Steam library path. ``--gibbed-tools``
defaults to ``$JC3_GIBBED_TOOLS``. ``--output`` defaults to ``sights.json`` beside this script.
``--dump`` writes each entity's embedded files and the ``.epe`` in use under ``DIR/<weapon>/``.
``--plots`` writes a side view per weapon with the derived line (requires matplotlib).
"""

import argparse
import json
import logging
import os
import re
import sys
from pathlib import Path

import plot
import sightline
import weapon
from archives import GameArchives, read_entity

log = logging.getLogger("sights")

DEFAULT_GAME_DIR = Path.home() / ".local/share/Steam/steamapps/common/Just Cause 3"
FILE_LISTS = Path("bin/projects/Just Cause 3/files")
PLAYER_WEAPON = re.compile(r"^editor/entities/jc_weapons/(0[12]_[a-z_]+)/[^/]+/([^/]+)\.ee$")
ONE_HANDED = "01_one_handed"

# The confidence grades, from most to least trustworthy:
# - scoped: a scope tube was found, and the line runs down its axis;
# - pistol: a one-handed weapon's iron sights, whose line passes the plausibility checks;
# - iron_uncertain: an iron-sight line on a two-handed weapon, or one that fails the checks;
# - failed: no line was derived, and the entry carries no correction.
GRADES = ("scoped", "pistol", "iron_uncertain", "failed")


def main() -> int:
    args = _parse_args()
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s: %(message)s", stream=sys.stderr)
    if args.gibbed_tools is None:
        log.error("the Gibbed tools path is not set: pass --gibbed-tools or set JC3_GIBBED_TOOLS")
        return 2

    try:
        entities = _player_weapon_entities(args.gibbed_tools / FILE_LISTS)
        archives = GameArchives(args.game)
    except (OSError, ValueError) as e:
        log.error("%s", e)
        return 1
    log.info("found %d player weapon entities", len(entities))

    table = []
    for path in entities:
        entry, loaded, line = _derive(path, archives)
        table.append(entry)
        if args.dump:
            _dump(path, archives, args.dump / entry["weapon"])
        if args.plots and loaded is not None:
            plot.side_view(loaded, line, args.plots / f"{entry['weapon']}.png")

    table.sort(key=lambda e: e["weapon"])
    # One entry per line, so that a regenerated table diffs by weapon.
    args.output.write_text("[\n" + ",\n".join("  " + json.dumps(e) for e in table) + "\n]\n")
    counts = {grade: sum(e["grade"] == grade for e in table) for grade in GRADES}
    log.info("wrote %d entries to %s (%s)", len(table), args.output, ", ".join(f"{g}: {n}" for g, n in counts.items()))
    return 0


def _parse_args():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    env_game = os.environ.get("JC3_GAME_DIR")
    env_tools = os.environ.get("JC3_GIBBED_TOOLS")
    parser.add_argument("--game", type=_path, default=_path(env_game) if env_game else DEFAULT_GAME_DIR, help="the game install directory")
    parser.add_argument("--gibbed-tools", type=_path, default=_path(env_tools) if env_tools else None, help="the Gibbed Just Cause 3 tools checkout")
    parser.add_argument("--output", type=_path, default=Path(__file__).resolve().parent / "sights.json")
    parser.add_argument("--dump", type=_path, help="write each entity's files under this directory")
    parser.add_argument("--plots", type=_path, help="write side-view PNGs under this directory")
    return parser.parse_args()


def _path(text: str) -> Path:
    return Path(os.path.expanduser(os.path.expandvars(text)))


def _player_weapon_entities(file_lists: Path) -> list[str]:
    lists = sorted(file_lists.glob("**/*.filelist"))
    if not lists:
        raise FileNotFoundError(f"file lists: no *.filelist under {file_lists}")
    names = set()
    for path in lists:
        for line in path.read_text(errors="replace").splitlines():
            name = line.strip().replace("\\", "/").lower()
            # The magazine entities sit beside the weapons and are not weapons.
            if PLAYER_WEAPON.match(name) and not name.endswith("_mag.ee"):
                names.add(name)
    return sorted(names)


def _derive(path: str, archives: GameArchives):
    """The table entry for one entity, with the loaded weapon and sight line when they exist."""
    class_dir, name = PLAYER_WEAPON.match(path).groups()
    one_handed = class_dir == ONE_HANDED
    entry = {
        "weapon": name,
        "weapon_hash": weapon.name_hash(path),
        "offset_m": [0.0, 0.0, 0.0],
        "angles_deg": [0.0, 0.0],
        "grade": "failed",
    }
    try:
        loaded = weapon.load(path, archives)
        line = sightline.derive(loaded)
    except (LookupError, ValueError) as e:
        log.warning("%s: failed: %s", entry["weapon"], e)
        return entry, None, None

    if loaded.stored_name_hash != entry["weapon_hash"]:
        log.warning("%s: the .epe's name_hash %#010x differs from the entity name's hash %#010x", entry["weapon"], loaded.stored_name_hash, entry["weapon_hash"])
    if len({fp.translation for fp in loaded.fire_points}) > 1:
        log.warning("%s: %d distinct fire positions; using the first", entry["weapon"], len(loaded.fire_points))

    if line.kind == "scope":
        grade = "scoped"
    elif one_handed and line.doubt is None:
        grade = "pistol"
    else:
        grade = "iron_uncertain"
    entry["offset_m"] = [0.0, round(line.height_at_muzzle, 4), 0.0]
    entry["angles_deg"] = [round(line.pitch_deg, 3), 0.0]
    entry["grade"] = grade
    log.info(
        "%s %#010x %s: height %.4f m, pitch %.2f deg (.epe: %s)%s",
        entry["weapon"],
        entry["weapon_hash"],
        grade,
        line.height_at_muzzle,
        line.pitch_deg,
        loaded.epe_source,
        f"; {line.doubt}" if line.doubt else "",
    )
    return entry, loaded, line


def _dump(path: str, archives: GameArchives, out: Path) -> None:
    data = archives.read(path)
    if data is None:
        return
    files = read_entity(data)
    epe_path = path.removesuffix(".ee") + ".epe"
    standalone = archives.read(epe_path)
    if standalone is not None:
        files[epe_path] = standalone
    for name, content in files.items():
        target = out / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)


if __name__ == "__main__":
    sys.exit(main())
