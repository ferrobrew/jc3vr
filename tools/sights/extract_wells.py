#!/usr/bin/env python3
"""Derive the player weapons' reload geometry from the game's models and write a magazine well table.

For every player weapon entity, the same set as ``extract_sights.py`` covers, this loads the closest
LOD of every model part, the magazine included, and derives where the magazine, shell, or round goes
in and where the hand grabs to chamber, in weapon space (see ``wells.py``). For a magazine-fed
weapon whose spare magazine prop depicts the magazine a hand loads, it also measures the top of that
prop's model in the prop's own frame (see ``spares.py``). For a shell-fed weapon or a revolver, which
have no model of a loose round, it names the casing model their ejection effect throws out and
measures it, as the stand-in a round in the hand is drawn with (see ``rounds.py``). A gun whose
magazine is modelled into its body with no prop borrows the spare magazine of the weapon whose
prop matches it best (see ``borrowed.py``).

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

import borrowed
import rounds
import spares
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
        archives = GameArchives(args.game)
        entities = _player_weapon_entities(args.gibbed_tools / FILE_LISTS, archives)
        spare_magazines = spares.SpareMagazines(archives)
    except (OSError, ValueError, LookupError) as e:
        log.error("%s", e)
        return 1
    log.info("found %d player weapon entities", len(entities))

    table = []
    derived = []
    for path in entities:
        entry, loaded, well, spare = _derive(path, archives, spare_magazines)
        table.append(entry)
        derived.append((entry, loaded, well, spare))
        if args.dump:
            _dump(path, archives, args.dump / entry["weapon"])
        if args.plots and loaded is not None:
            wells_plot.views(loaded, well, args.plots / f"{entry['weapon']}.png")
    donors = [spare for *_, spare in derived if spare is not None]
    for entry, loaded, well, spare in derived:
        _borrow(entry, loaded, well, donors)

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


def _derive(path: str, archives: GameArchives, spare_magazines: spares.SpareMagazines):
    """The table entry for one entity, with the loaded weapon and well when they exist."""
    name = PLAYER_WEAPON.match(path).group(2)
    try:
        # Every part, the magazine included: the magazine is what this tool measures.
        loaded = weapon.load(path, archives, skipped_part_names=())
    except (LookupError, ValueError) as e:
        well = wells.Well(wells.NONE, wells.FAILED, doubts=[f"the model could not be read: {e}"])
        loaded = None
    else:
        try:
            well = wells.derive(loaded, wells.family(name, loaded.category))
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
    spare = _spare(name, loaded, well, spare_magazines)
    entry["spare_top_m"] = _vec(spare and spares.top_centre(spare.positions, spare.axis))
    entry["spare_from"] = None
    entry["spare_origin_m"] = None
    entry.update(_round(name, path, well, archives))
    if entry["round_count"] is not None and well.kind == wells.CYLINDER:
        entry["notes"] = "; ".join(filter(None, (entry.get("notes"), "round_count: class default")))

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
    return entry, loaded, well, spare


def _borrow(entry: dict, loaded: weapon.Weapon | None, well: wells.Well, donors: list[borrowed.Donor]):
    """Give a gun whose magazine is modelled below its receiver, with no spare magazine prop of its
    own, the best matching donor's spare: its tag, its top, and where its origin sits seated."""
    if (
        loaded is None
        or well.kind != wells.MAGAZINE
        or well.part is not None
        or well.part_length is None
        or loaded.magazine_tag is not None
        or well.mouth is None
        or well.axis is None
    ):
        return
    extents = borrowed.built_in_extents(loaded, well.mouth, well.axis, well.part_length)
    choice = extents and borrowed.choose(extents, well.axis, well.mouth, donors)
    if not choice:
        log.warning("%s: no spare magazine to borrow", entry["weapon"])
        return
    entry["spare_top_m"] = _vec(spares.top_centre(choice.donor.positions, choice.donor.axis))
    entry["spare_from"] = choice.donor.tag
    entry["spare_origin_m"] = _vec(choice.origin)
    note = f"spare: borrowed from {choice.donor.weapon}, the best match by size"
    entry["notes"] = "; ".join(filter(None, (entry.get("notes"), note)))
    log.info(
        "%s: borrows %s's spare magazine (built in %s m, mismatch %.3f m)",
        entry["weapon"],
        choice.donor.weapon,
        [round(e, 3) for e in extents],
        choice.mismatch,
    )


def _spare(name: str, loaded: weapon.Weapon | None, well: wells.Well, spare_magazines: spares.SpareMagazines) -> borrowed.Donor | None:
    """The spare magazine's model and the insertion axis in its frame (taken into the magazine
    part's frame when the weapon has one), when the spare depicts the magazine a hand loads: the
    spare's model is the weapon's magazine part, or the weapon has no round part and its magazine is
    modelled into the body."""
    if loaded is None or well.kind != wells.MAGAZINE or well.axis is None:
        return None
    try:
        spare = spare_magazines.model(loaded)
    except (LookupError, ValueError) as e:
        log.warning("%s: %s", name, e)
        return None
    if spare is None:
        return None
    round_part = next((p for n in wells.ROUND_PARTS for p in loaded.parts if p.name == n), None)
    if round_part is not None and not (round_part.name == "model.magazine" and round_part.model == spare.model):
        log.info("%s: the spare magazine %s is not the round part %s", name, spare.model, round_part.model)
        return None
    axis = well.axis
    if round_part is not None and round_part.matrix is not None:
        # The axis is in weapon space, and the part's rows are its own axes in weapon space, so the
        # axis's coordinates in the part's (and the spare model's) frame are its projections on them.
        m = round_part.matrix
        axis = tuple(sum(axis[j] * m[4 * i + j] for j in range(3)) for i in range(3))
    return borrowed.Donor(name, loaded.magazine_tag, spare.positions, axis)


def _round(name: str, path: str, well: wells.Well, archives: GameArchives) -> dict:
    """The stand-in round model's fields: the casing a shell-fed weapon or a revolver ejects, one
    for a shell and a cylinder's worth for a revolver; all null for any other weapon."""
    fields = dict.fromkeys(("round_model", "round_top_m", "round_axis", "round_radius_m", "round_count", "round_ring_m"))
    count = {wells.SHELL: 1, wells.CYLINDER: rounds.REVOLVER_CHAMBERS}.get(well.kind)
    if count is None:
        return fields
    try:
        found = rounds.resolve(path, archives, count)
    except (LookupError, ValueError) as e:
        log.warning("%s: %s", name, e)
        return fields
    fields.update(
        round_model=found.model,
        round_top_m=_vec(found.top),
        round_axis=_vec(found.axis),
        round_radius_m=_num(found.radius),
        round_count=found.count,
        round_ring_m=_num(found.ring),
    )
    return fields


def _vec(v) -> list[float] | None:
    # Adding 0.0 turns a rounded -0.0 into 0.0.
    return None if v is None else [round(c, DECIMALS) + 0.0 for c in v]


def _num(v) -> float | None:
    return None if v is None else round(v, DECIMALS) + 0.0


if __name__ == "__main__":
    sys.exit(main())
