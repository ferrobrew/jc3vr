#!/usr/bin/env python3
"""Decode the character animation state machines (``.afsmb``) into readable text.

For each graph this writes three files into the output directory:

- ``<graph>.conds.txt``: every condition with its class and its other properties;
- ``<graph>.graph.txt``: the initial state, the global and external transitions, and every
  state's transitions, one per line as ``STATE [EVENT] conditions -> DESTINATION`` (``!`` marks a
  condition that must be false; ``&`` joins all-of conditions and ``|`` any-of);
- ``<graph>.tasks.txt``: every state with its class and its state tasks and their properties.

Usage::

    decode_graph.py --out DIR [--game DIR] [--dump DIR] [--graph PATH ...] [--file FILE ...]

``--game`` defaults to ``$JC3_GAME_DIR``, then to the default Steam library path; the graphs are
read from its archives, the patch copy in preference. ``--graph`` names an archive path and defaults
to Rico's four graphs. ``--file`` decodes a local ``.afsmb`` instead. ``--dump`` defaults to
``$JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP``: the condition and state class names are recovered by
hashing the class names found there, and the task type names come from its state-task registration
table; without it, classes and task types print as hashes.
"""

import argparse
import logging
import os
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "sights"))

import afsmb  # noqa: E402
from archives import GameArchives, hashlittle  # noqa: E402

log = logging.getLogger("anim_graph")

DEFAULT_GAME_DIR = Path.home() / ".local/share/Steam/steamapps/common/Just Cause 3"
RICO = "animations/jc_statemachine/main_characters/rico"
DEFAULT_GRAPHS = [f"{RICO}/{name}.afsmb" for name in ("rico_base", "rico_partial_sync", "rico_partial_global", "rico_additive_sync")]

# An entry of the state-task registration table: ``u32``, the type hash as ``u32``, then the name.
TASK_REGISTRATION = re.compile(r"(\d+)u,\s*\n\s*\"CStateTask_(\w+)\"")


def main() -> int:
    args = _parse_args()
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s: %(message)s", stream=sys.stderr)
    classes, task_types = _names(args.dump)

    sources: list[tuple[str, bytes]] = []
    try:
        for path in args.file:
            sources.append((path.stem, path.read_bytes()))
        if not args.file:
            archives = GameArchives(args.game)
            for name in args.graph:
                data = archives.read(name)
                if data is None:
                    log.error("%s: not in the archives", name)
                    return 1
                sources.append((Path(name).stem, data))
    except (OSError, ValueError) as e:
        log.error("%s", e)
        return 1

    args.out.mkdir(parents=True, exist_ok=True)
    for stem, data in sources:
        try:
            machine = afsmb.parse(data)
        except (ValueError, IndexError) as e:
            log.error("%s: %s", stem, e)
            return 1
        _write(args.out, stem, machine, classes, task_types)
        transitions = sum(len(t) for t in machine.state_transitions.values())
        log.info("%s: %d states, %d conditions, %d state transitions", stem, len(machine.states), len(machine.conditions), transitions)
    return 0


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--out", type=Path, required=True, help="the directory to write the decoded graphs into")
    parser.add_argument("--game", type=Path, default=Path(os.environ.get("JC3_GAME_DIR", DEFAULT_GAME_DIR)))
    dump = os.environ.get("JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP")
    parser.add_argument("--dump", type=Path, default=Path(dump).expanduser() if dump else None)
    parser.add_argument("--graph", action="append", help="an archive path of a graph (repeatable)")
    parser.add_argument("--file", type=Path, action="append", default=[], help="a local .afsmb to decode (repeatable)")
    args = parser.parse_args()
    args.graph = args.graph or DEFAULT_GRAPHS
    return args


def _names(dump: Path | None) -> tuple[dict[int, str], dict[int, str]]:
    """Class names by hash, and task type names by type hash, from the symbol dump."""
    if dump is None or not dump.is_dir():
        log.warning("no symbol dump: class and task type names print as hashes")
        return {}, {}
    # The dump names its type and function files after their classes, in a subdirectory per
    # namespace (``NAnimationSystem/CAnimationState.h``); a class hash is of the bare name.
    classes = {}
    for _, _, files in os.walk(dump):
        for file in files:
            stem = file.rsplit(".", 1)[0]
            if stem.startswith("C") and stem.isidentifier():
                classes[hashlittle(stem)] = stem
    task_types = {}
    registration = dump / "_data.cpp"
    if registration.is_file():
        for type_hash, name in TASK_REGISTRATION.findall(registration.read_text(errors="replace")):
            task_types[int(type_hash)] = name
    else:
        log.warning("%s: missing; task type names print as hashes", registration)
    log.info("recovered %d class names and %d task type names", len(classes), len(task_types))
    return classes, task_types


def _write(out: Path, stem: str, machine: afsmb.StateMachine, classes: dict[int, str], task_types: dict[int, str]) -> None:
    def cls(h: int) -> str:
        return classes.get(h, f"{h:#010x}")

    with open(out / f"{stem}.conds.txt", "w") as f:
        for name, class_hash, properties in machine.conditions:
            f.write(f"{name} {cls(class_hash)} {_properties(properties, {})}\n")

    with open(out / f"{stem}.graph.txt", "w") as f:
        f.write(f"INITIAL {machine.initial}\n")
        for t in machine.global_transitions:
            f.write(f"GLOBAL {t.describe()}\n")
        for t in machine.external_transitions:
            f.write(f"EXTERNAL {t.describe()}\n")
        for state, transitions in machine.state_transitions.items():
            if not transitions:
                f.write(f"{state}\n")
            for t in transitions:
                f.write(f"{state} {t.describe()}\n")

    with open(out / f"{stem}.tasks.txt", "w") as f:
        for state, class_hash in machine.states:
            tasks = machine.state_tasks.get(state, [])
            described = " ; ".join(
                f"{task_types.get(t, f'{t:#010x}')}({_properties(p, afsmb.TASK_PROPERTY_NAMES)})" for t, p in tasks
            )
            f.write(f"{state} {cls(class_hash)} {described or '-'}\n")


def _properties(properties: list[tuple[int, object]], names: dict[int, str]) -> str:
    def value(v: object) -> str:
        return v.hex() if isinstance(v, bytes) else str(v)

    return ", ".join(f"{names.get(k, f'{k:#x}')}={value(v)}" for k, v in properties)


if __name__ == "__main__":
    sys.exit(main())
