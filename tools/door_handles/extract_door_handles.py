#!/usr/bin/env python3
"""Derive every vehicle door's handle from the game's entry clips and write a door handle table.

For every vehicle entity (``editor/entities/jc_vehicles/**.ee``) this reads the vehicle's parts,
deform points, and interaction graphs. Each entry of a graph that the player can take names a
state and, with the graph's or the entry's modifier, an animation node of the player's animation
set (``rico_base.asb``): ``ANIM_<state>_<modifier>``, or ``ANIM_<state>`` without one. The node's
clip and its ``open_door`` track message give the moment the hand opens the door; the clip, warped
onto the entry's destination node, gives where the hand is then (see ``handles.py``). That hand is
matched to the nearest door part and projected onto the door's outer surface. NPC entries are
tried against the soldiers' animation set the same way. Doors no clip reaches, such as hoods and
trunks, get a handle from their box and their constraint point.

Usage::

    extract_door_handles.py [--game DIR] [--gibbed-tools DIR] [--output FILE] [--self-check]

``--game`` defaults to ``$JC3_GAME_DIR``, then to the default Steam library path. ``--gibbed-tools``
defaults to ``$JC3_GIBBED_TOOLS``. ``--output`` defaults to ``door_handles.json`` beside this
script. ``--self-check`` checks the geometry on made-up doors and needs no game data.
"""

import argparse
import json
import logging
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "sights"))

import handles  # noqa: E402
from animset import AnimNode, anim_nodes  # noqa: E402
from archives import GameArchives, read_entity  # noqa: E402
from havok import Animation, Skeleton, bone_model_transforms  # noqa: E402
from vehicle import Door, Vehicle, load  # noqa: E402
from xform import Xform, length, sub  # noqa: E402

log = logging.getLogger("door_handles")

DEFAULT_GAME_DIR = Path.home() / ".local/share/Steam/steamapps/common/Just Cause 3"
FILE_LISTS = Path("bin/projects/Just Cause 3/files")
VEHICLE = re.compile(r"^editor/entities/jc_vehicles/.+\.ee$")


@dataclass(frozen=True)
class CharacterSet:
    """One character's animations: its animation set, skeleton, and the entity that holds its
    clips."""

    label: str
    asb: str
    skeleton: str
    entity: str


PLAYER = CharacterSet(
    "player",
    "animations/jc_statemachine/main_characters/rico/rico_base.asb",
    "animations/jc_skeletons/characters/rico.bsk",
    "editor/entities/gameobjects/main_character.ee",
)
NPC = CharacterSet(
    "npc",
    "animations/jc_statemachine/npc/enemies/soldier_gen/soldier_gen_base.asb",
    "animations/jc_skeletons/characters/gen_male.bsk",
    "editor/entities/jc_characters/enemies/02_army/assault_trooper/assault_trooper.ee",
)

# The entry states whose clips open a door by hand, in order of preference: the standard entries,
# then the side, hijack, and unlock entries.
PREFERRED_STATES = ("S_ENTER_DRIVER_VEHICLE", "S_ENTER_PASSENGER_VEHICLE")
OPEN_MESSAGES = ("open_door", "open_door_with_identifier", "quick_open_door")
# A clip's door opens at its start when its open message is this early: the hand is not on it.
OPENS_AT_START_S = 1e-3
# A hand farther than this from its door's box is doubtful, metres, and one farther than the
# second is not on the door at all.
DOUBTFUL_HAND_TO_DOOR_M = 0.1
MAX_HAND_TO_DOOR_M = 0.3
# A projection onto the surface longer than this is doubtful, metres.
DOUBTFUL_GAP_M = 0.2
GRADES = ("clip", "clip_doubtful", "hinge")


def main() -> int:
    args = _parse_args()
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s: %(message)s", stream=sys.stderr)
    if args.self_check:
        failures = handles.self_check()
        for f in failures:
            log.error("self-check failed: %s", f)
        log.info("self-check: %d failure(s)", len(failures))
        return 1 if failures else 0
    if args.gibbed_tools is None:
        log.error("the Gibbed tools path is not set: pass --gibbed-tools or set JC3_GIBBED_TOOLS")
        return 2
    try:
        entities = _vehicle_entities(args.gibbed_tools / FILE_LISTS)
        archives = GameArchives(args.game)
        characters = {c.label: _Character(c, archives) for c in (PLAYER, NPC)}
    except (OSError, ValueError, LookupError) as e:
        log.error("%s", e)
        return 1
    log.info("found %d vehicle entities", len(entities))

    table: dict[int, dict] = {}
    seen_models: set[str] = set()
    for entity in entities:
        try:
            vehicle = load(entity, archives)
        except (LookupError, ValueError, KeyError) as e:
            log.warning("%s: skipped: %s", _short(entity), e)
            continue
        if not vehicle.doors:
            continue
        # The variants of a model share its parts; only the first is logged, and the others are
        # checked against it.
        verbose = vehicle.model not in seen_models
        seen_models.add(vehicle.model)
        for entry in _vehicle_handles(vehicle, characters, verbose):
            previous = table.get(entry["part_hash"])
            if previous is None:
                table[entry["part_hash"]] = entry
            elif length(sub(previous["handle_m"], entry["handle_m"])) > 0.01:
                log.warning("%s: %s: %s gives %s, %s gave %s; keeping the first", _short(entity), entry["part"], entry["entity"], entry["handle_m"], previous["entity"], previous["handle_m"])

    rows = sorted(table.values(), key=lambda e: (e["vehicle"], e["part"]))
    args.output.write_text("[\n" + ",\n".join("  " + json.dumps(r) for r in rows) + "\n]\n")
    counts = {g: sum(r["grade"] == g for r in rows) for g in GRADES}
    models = len({r["vehicle"] for r in rows})
    log.info("wrote %d doors of %d vehicle models to %s (%s)", len(rows), models, args.output, ", ".join(f"{g}: {n}" for g, n in counts.items()))
    return 0


class _Character:
    def __init__(self, spec: CharacterSet, archives: GameArchives):
        self.spec = spec
        entity = archives.read(spec.entity)
        self.files = read_entity(entity) if entity else {}
        asb = archives.read(spec.asb) or self.files.get(spec.asb)
        skeleton = archives.read(spec.skeleton) or self.files.get(spec.skeleton)
        if asb is None or skeleton is None:
            raise LookupError(f"{spec.label}: the animation set or the skeleton is missing")
        self.nodes = anim_nodes(asb)
        self.skeleton = Skeleton(skeleton, spec.skeleton)
        self.archives = archives
        self._clips: dict[str, Animation | None] = {}

    def clip(self, path: str) -> Animation | None:
        if path not in self._clips:
            data = self.files.get(path) or self.archives.read(path)
            self._clips[path] = Animation(data, path) if data else None
        return self._clips[path]


@dataclass
class _Reach:
    door: Door
    hand: str
    palm_vehicle: tuple
    distance: float
    entry: str
    state: str
    node: AnimNode
    clip: str
    open_s: float
    warped: bool
    preference: int


def _vehicle_handles(vehicle: Vehicle, characters: dict[str, "_Character"], verbose: bool) -> list[dict]:
    report = log.info if verbose else log.debug
    reaches: dict[str, _Reach] = {}
    for g in vehicle.graphs:
        for entry in g.entries:
            for reach in _entry_reaches(vehicle, g, entry, characters):
                best = reaches.get(reach.door.part)
                if best is None or (reach.preference, reach.distance) < (best.preference, best.distance):
                    reaches[reach.door.part] = reach
    # A side door without a clip takes its handle's height from a side door that has one.
    side_heights = [r.palm_vehicle[1] for r in reaches.values() if handles.kind_of(r.door.behaviour) == handles.SIDE]
    matched_height = sum(side_heights) / len(side_heights) if side_heights else None
    out = []
    for door in vehicle.doors:
        if door.mesh is None:
            if verbose:
                log.warning("%s: %s: no model, so no handle", vehicle.model, door.part)
            continue
        box = handles.Box(tuple(door.mesh.box_min), tuple(door.mesh.box_max))
        kind = handles.kind_of(door.behaviour)
        reach = reaches.get(door.part)
        row = {"vehicle": vehicle.model, "part": door.part, "part_hash": door.part_hash, "slot": door.behaviour}
        if reach is not None:
            palm = door.to_vehicle.inverse().apply(reach.palm_vehicle)
            _thickness, outward = handles.outward_axis(box, door.to_vehicle, kind)
            hit = handles.surface_point(palm, outward, box, door.mesh.triangles)
            gap = length(sub(hit.point, palm))
            doubts = []
            if reach.state not in PREFERRED_STATES:
                doubts.append(f"from {reach.state}")
            if not reach.warped:
                doubts.append("the warp has not finished at the open message")
            if reach.distance > DOUBTFUL_HAND_TO_DOOR_M:
                doubts.append(f"the hand is {reach.distance:.3f} m outside the door's box")
            if gap > DOUBTFUL_GAP_M:
                doubts.append(f"the palm is {gap:.3f} m from the surface")
            if not hit.on_mesh:
                doubts.append("the door's mesh could not be read")
            row.update(
                handle_m=_round(hit.point),
                normal=_round(hit.normal),
                grade="clip_doubtful" if doubts else "clip",
                entity=_short(vehicle.entity),
                entry=reach.entry,
                clip=reach.clip.rsplit("/", 1)[-1],
                hand=reach.hand,
                open_s=round(reach.open_s, 4),
                palm_m=_round(palm),
                notes="; ".join(doubts),
            )
            report("%s %s: %s from %s (%s hand at %.3f s), %.3f m to the surface%s", vehicle.model, door.part, row["grade"], row["clip"], reach.hand, reach.open_s, gap, f"; {row['notes']}" if doubts else "")
        else:
            point, _thickness, outward, rule = handles.hinge_handle(box, door.to_vehicle, kind, door.hinge, matched_height if kind == handles.SIDE else None)
            hit = handles.surface_point(point, outward, box, door.mesh.triangles)
            notes = [f"hinge from the {rule}"]
            if kind == handles.SIDE and matched_height is not None:
                notes.append("height from the clip-derived side doors")
            row.update(handle_m=_round(hit.point), normal=_round(hit.normal), grade="hinge", entity=_short(vehicle.entity), notes="; ".join(notes))
            report("%s %s: hinge (%s)", vehicle.model, door.part, row["notes"])
        out.append(row)
    return out


def _entry_reaches(vehicle: Vehicle, g, entry, characters: dict[str, "_Character"]):
    if not entry.state or not entry.state.startswith("S_ENTER") or entry.node is None:
        return
    character = characters["npc" if entry.user_type == "npc" else "player"]
    modifier = entry.modifier or entry.node.modifier or g.modifier
    base = "ANIM_" + entry.state.removeprefix("S_")
    node = character.nodes.get(f"{base}_{modifier}") if modifier else None
    node = node or character.nodes.get(base)
    if node is None:
        return
    opens = [m for m in node.messages if m.name in OPEN_MESSAGES]
    if not opens:
        return
    message = opens[0]
    open_s = message.time
    if open_s <= OPENS_AT_START_S:
        log.debug("%s %s: %s opens its door at the clip's start", vehicle.model, entry.name, node.name)
        return
    # The door the message opens: the slot it names, or the entry's identifier, which is the
    # slot of the seat it leads to.
    slot = message.argument if message.name == "open_door_with_identifier" else entry.identifier
    if not isinstance(slot, int) or not 0 <= slot < len(handles.DOOR_SLOTS):
        log.debug("%s %s: %s opens no known door slot (%r)", vehicle.model, entry.name, node.name, slot)
        return
    doors = [d for d in vehicle.doors if d.behaviour == handles.DOOR_SLOTS[slot] and d.mesh is not None]
    if not doors:
        return
    destination = _destination(vehicle, entry)
    if destination is None:
        log.debug("%s %s: the destination node %s has no transform", vehicle.model, entry.name, entry.node.name)
        return
    clip = character.clip(node.clips[0])
    if clip is None:
        log.warning("%s %s: the clip %s is missing", vehicle.model, entry.name, node.clips[0])
        return
    progress = handles.warp_progress(node.segments, open_s)
    if progress is None:
        progress = handles.WarpProgress(1.0, 1.0, 1.0)
    skeleton = character.skeleton
    anchor_name = entry.alignment_bone or "Reference"
    if anchor_name not in skeleton.index:
        log.warning("%s %s: no alignment bone %s in %s", vehicle.model, entry.name, anchor_name, character.spec.skeleton)
        return
    try:
        at_open = bone_model_transforms(clip, skeleton, open_s)
        at_end = bone_model_transforms(clip, skeleton, clip.duration)
    except ValueError as e:
        log.warning("%s %s: %s", vehicle.model, entry.name, e)
        return
    anchor_end = at_end[skeleton.index[anchor_name]]
    preference = PREFERRED_STATES.index(entry.state) if entry.state in PREFERRED_STATES else len(PREFERRED_STATES)
    if character.spec.label == "npc":
        preference += len(PREFERRED_STATES) + 1
    best = None
    for hand, bone in handles.HANDS:
        if bone not in skeleton.index:
            continue
        palm = handles.bone_in_vehicle(at_open[skeleton.index[bone]], anchor_end, destination, progress, vehicle.ground_y)
        for door in doors:
            d = handles.Box(tuple(door.mesh.box_min), tuple(door.mesh.box_max)).distance(door.to_vehicle.inverse().apply(palm))
            if best is None or d < best[2]:
                best = (door, hand, d, palm)
    if best is not None and best[2] > MAX_HAND_TO_DOOR_M:
        log.debug("%s %s: the %s hand is %.3f m from %s", vehicle.model, entry.name, best[1], best[2], best[0].part)
    elif best is not None:
        door, hand, d, palm = best
        yield _Reach(door, hand, palm, d, entry.name, entry.state, node, node.clips[0], open_s, progress.complete_in_plane, preference)


def _destination(vehicle: Vehicle, entry) -> Xform | None:
    node = entry.node
    if node.deform_point:
        return vehicle.deform_points.get(node.deform_point)
    if node.world is not None:
        return Xform.from_matrix(node.world)
    return None


def _round(v) -> list[float]:
    return [round(c, 4) for c in v]


def _short(entity: str) -> str:
    return entity.rsplit("/", 1)[-1].removesuffix(".ee")


def _vehicle_entities(file_lists: Path) -> list[str]:
    lists = sorted(file_lists.glob("**/*.filelist"))
    if not lists:
        raise FileNotFoundError(f"file lists: no *.filelist under {file_lists}")
    names = set()
    for path in lists:
        for line in path.read_text(errors="replace").splitlines():
            name = line.strip().replace("\\", "/").lower()
            if VEHICLE.match(name):
                names.add(name)
    # The debug variants come last, so that a shipped variant's result is the one kept.
    return sorted(names, key=lambda n: ("_debug" in n, n))


def _parse_args():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    env_game = os.environ.get("JC3_GAME_DIR")
    env_tools = os.environ.get("JC3_GIBBED_TOOLS")
    parser.add_argument("--game", type=_path, default=_path(env_game) if env_game else DEFAULT_GAME_DIR, help="the game install directory")
    parser.add_argument("--gibbed-tools", type=_path, default=_path(env_tools) if env_tools else None, help="the Gibbed Just Cause 3 tools checkout")
    parser.add_argument("--output", type=_path, default=Path(__file__).resolve().parent / "door_handles.json")
    parser.add_argument("--self-check", action="store_true", help="check the geometry on made-up doors and exit")
    return parser.parse_args()


def _path(text: str) -> Path:
    return Path(os.path.expanduser(os.path.expandvars(text)))


if __name__ == "__main__":
    sys.exit(main())
