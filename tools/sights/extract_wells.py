#!/usr/bin/env python3
"""Derive the player weapons' reload geometry from the game's models and write a magazine well table.

For every player weapon entity, the same set as ``extract_sights.py`` covers, this loads the closest
LOD of every model part, the magazine included, and derives where the magazine, shell, or round goes
in and where the hand grabs to chamber, in weapon space (see ``wells.py``).

Usage::

    extract_wells.py [--game DIR] [--gibbed-tools DIR] [--output FILE] [--dump DIR] [--plots DIR]

``--game`` defaults to ``$JC3_GAME_DIR``, then to the default Steam library path. ``--gibbed-tools``
defaults to ``$JC3_GIBBED_TOOLS``. ``--output`` defaults to ``wells.json`` beside this script.
``--dump`` writes each entity's embedded files and the ``.epe`` in use under ``DIR/<weapon>/``.
``--plots`` writes a side, top, and front view per weapon with the derived points (requires
matplotlib).
"""

import argparse
import json
import logging
import os
import sys
from pathlib import Path

import weapon
import wells
import wells_plot
from archives import GameArchives
from extract_sights import DEFAULT_GAME_DIR, FILE_LISTS, PLAYER_WEAPON, _dump, _path, _player_weapon_entities

log = logging.getLogger("wells")

GRADES = (wells.PART, wells.MESH, wells.DEFAULT, wells.FAILED)
DECIMALS = 4


def main() -> int:
    args = _parse_args()
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s: %(message)s", stream=sys.stderr)
    if args.gibbed_tools is None:
        log.error("the Gibbed tools path is not set: pass --gibbed-tools or set JC3_GIBBED_TOOLS")
        return 2
    if args.plots and not wells_plot.AVAILABLE:
        log.error("plots: matplotlib is not installed")
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
        entry, loaded, well = _derive(path, archives)
        table.append(entry)
        if args.dump:
            _dump(path, archives, args.dump / entry["weapon"])
        if args.plots and loaded is not None:
            wells_plot.views(loaded, well, args.plots / f"{entry['weapon']}.png")

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
    parser.add_argument("--output", type=_path, default=Path(__file__).resolve().parent / "wells.json")
    parser.add_argument("--dump", type=_path, help="write each entity's files under this directory")
    parser.add_argument("--plots", type=_path, help="write review PNGs under this directory")
    return parser.parse_args()


def _derive(path: str, archives: GameArchives):
    """The table entry for one entity, with the loaded weapon and well when they exist."""
    name = PLAYER_WEAPON.match(path).group(2)
    family = name.split("_")[1]
    try:
        # Every part, the magazine included: the magazine is what this tool measures.
        loaded = weapon.load(path, archives, skipped_part_names=())
    except (LookupError, ValueError) as e:
        well = wells.Well(wells.NONE, wells.FAILED, doubts=[f"the model could not be read: {e}"])
        loaded = None
    else:
        try:
            well = wells.derive(loaded, family)
        except ValueError as e:
            # A heuristic that finds nothing to measure (an empty profile or cluster) fails this
            # weapon only.
            well = wells.Well(wells.NONE, wells.FAILED, doubts=[f"the derivation failed: {e}"])

    entry = {
        "weapon": name,
        "weapon_hash": weapon.name_hash(path),
        "kind": well.kind,
        "mouth_m": _vec(well.mouth),
        "insert_axis": _vec(well.axis),
        "magazine_part": well.part is not None,
        "part_name": well.part.name if well.part is not None else None,
        "part_top_m": _num(well.part_top),
        "part_length_m": _num(well.part_length),
        "action": well.action,
        "handle_m": _vec(well.handle),
        "travel_m": _num(well.travel),
        "grade": well.grade,
    }
    remarks = well.notes + [f"doubt: {d}" for d in well.doubts]
    if remarks:
        entry["notes"] = "; ".join(remarks)

    log.log(
        logging.WARNING if well.doubts or well.grade == wells.FAILED else logging.INFO,
        "%s %#010x %s/%s %s: mouth %s, axis %s, %s handle %s%s",
        name,
        entry["weapon_hash"],
        well.kind,
        well.grade,
        f"part {entry['part_name']}" if well.part is not None else "no part",
        entry["mouth_m"],
        entry["insert_axis"],
        well.action,
        entry["handle_m"],
        f"; doubtful: {', '.join(well.doubts)}" if well.doubts else "",
    )
    return entry, loaded, well


def _vec(v) -> list[float] | None:
    # Adding 0.0 turns a rounded -0.0 into 0.0.
    return None if v is None else [round(c, DECIMALS) + 0.0 for c in v]


def _num(v) -> float | None:
    return None if v is None else round(v, DECIMALS) + 0.0


if __name__ == "__main__":
    sys.exit(main())
