"""Print the draw census a capture recorded, with its shaders named.

A capture requested with ``draw_census`` carries, in its ``.capture.json``, one entry per group of
draw calls that agree on the dispatch, render pass, render-block type, bound shaders, entry point, and
the treatment the stereo draw path gave them. Vertex shaders arrive named; pixel shaders arrive as the
SHA-256 of the engine's bytecode and are named here from the shader bundle.

Usage::

    python -m tools.rendering census <capture.json> --bundle <Shaders_F.shader_bundle>
        [--passes FIRST:LAST] [--totals] [--accounting] [--multipass <capture.json>]

``--passes`` keeps the passes from ``FIRST`` to ``LAST`` inclusive, by ``RenderPassId`` name, with
the ids taken from the pyxis definitions.
``--totals`` adds the calls and submissions per treatment for the passes kept.
``--accounting`` prints, instead of the entries, the game's calls and the single-pass submissions by
handling, by render pass, by render-block type, and by render-block type and vertex shader, the last
two ranked by the submissions a single draw for both eyes would save (see ``draw_accounting``);
``--multipass`` adds, by pass and by block, the submissions a multipass census of the same scene
records.
"""

import argparse
import importlib.util
import json
import re
import sys
from collections import Counter
from pathlib import Path

from tools.rendering.comparison import draw_accounting as acc

SHADER_NAMES = Path(__file__).resolve().parents[2] / "shaders" / "shader_names.py"
RENDER_PASS_IDS = (Path(__file__).resolve().parents[3] / "jc3gi" / "pyxis-defs" / "projects" / "JustCause3" /
                   "Steam" / "20206564" / "graphics_engine" / "render_engine.pyxis")


class CensusError(Exception):
    pass


def bundle_names(bundle: Path) -> dict[str, str]:
    """Map each blob's SHA-256 to ``<type> <name>``."""
    spec = importlib.util.spec_from_file_location("shader_names", SHADER_NAMES)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    shaders, _ = module.name_shaders(bundle.read_bytes())
    return {s["pristine_sha256"]: f"{s['type']} {s['name']}" for s in shaders}


def shader_label(shader: dict, names: dict[str, str]) -> str:
    status = shader.get("status")
    if status == "named":
        return shader["name"]
    if status == "pristine":
        return names.get(shader["sha256"], shader["sha256"][:12]).split(" ", 1)[-1]
    if status == "unbound":
        return "-"
    return f"?{shader.get('reason', status)}"


def pass_ids(text: str) -> dict[str, int]:
    """The ``RenderPassId`` values the pyxis definitions give each pass name."""
    body = text.split("pub enum RenderPassId", 1)[-1].split("}", 1)[0]
    return {m.group(1): int(m.group(2), 0) for m in re.finditer(r"^\s*(\w+)\s*=\s*(0x[0-9A-Fa-f]+|\d+),", body, re.M)}


def select(entries: list[dict], passes: str | None, ids: dict[str, int] | None = None) -> list[dict]:
    if passes is None:
        return entries
    first, sep, last = passes.partition(":")
    if not sep:
        raise CensusError("census: --passes takes FIRST:LAST")
    ids = {**(ids or {}), **{e["pass_name"]: e["pass"] for e in entries}}
    missing = [name for name in (first, last) if name not in ids]
    if missing:
        raise CensusError(f"census: unknown pass {', '.join(missing)}")
    low, high = ids[first], ids[last]
    return [e for e in entries if low <= e["pass"] <= high]


def rows(entries: list[dict], names: dict[str, str]) -> list[str]:
    out = []
    for e in sorted(entries, key=lambda e: (e["dispatch"], e["pass"], e["block"] or "", -e["calls"])):
        eye = "" if e["reissue_eye"] is None else f" reissue_eye={e['reissue_eye']}"
        if e.get("split_eye") is not None:
            eye += f" split_eye={e['split_eye']}"
        patched = " patched" if e.get("vertex_patched") else ""
        out.append(f"{e['dispatch']} {e['pass_name']} {e['block'] or '-'} "
                   f"vs={shader_label(e['vertex_shader'], names)}{patched} ps={shader_label(e['pixel_shader'], names)} "
                   f"{e['entry']} {e['treatment']}{eye} calls={e['calls']} submissions={e['submissions']}")
    return out


def totals(entries: list[dict]) -> list[str]:
    calls, submissions = Counter(), Counter()
    for e in entries:
        calls[e["treatment"]] += e["calls"]
        submissions[e["treatment"]] += e["submissions"]
    return [f"{t}: calls={calls[t]} submissions={submissions[t]}" for t in sorted(calls)]


def accounting_report(entries: list[dict], multipass: list[dict] | None, names: dict[str, str]) -> list[str]:
    def label(shader: dict) -> str:
        return shader_label(shader, names)

    with_mp = multipass is not None
    out = acc.table(acc.account(entries, None, acc.by_handling, label), "handling", False, False)
    out += [""] + acc.table(acc.account(entries, multipass, acc.by_pass, label), "pass", False, with_mp)
    out += [""] + acc.table(acc.account(entries, multipass, acc.by_block, label), "block", True, with_mp)
    out += [""] + acc.table(acc.account(entries, None, acc.by_family, label), "shader", True, False)
    return out


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="python -m tools.rendering census")
    parser.add_argument("capture", type=Path)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--passes")
    parser.add_argument("--totals", action="store_true")
    parser.add_argument("--accounting", action="store_true")
    parser.add_argument("--multipass", type=Path)
    args = parser.parse_args(argv)
    try:
        census = json.loads(args.capture.read_text()).get("draw_census")
        if not isinstance(census, dict):
            raise CensusError(f"census: {args.capture} carries no draw census")
        ids = pass_ids(RENDER_PASS_IDS.read_text()) if RENDER_PASS_IDS.exists() else {}
        entries = select(census["entries"], args.passes, ids)
        names = bundle_names(args.bundle)
        multipass = None
        if args.multipass is not None:
            reference = json.loads(args.multipass.read_text()).get("draw_census")
            if not isinstance(reference, dict):
                raise CensusError(f"census: {args.multipass} carries no draw census")
            multipass = select(reference["entries"], args.passes, ids)
    except (CensusError, OSError, json.JSONDecodeError) as error:
        print(error, file=sys.stderr)
        return 1
    if census.get("error"):
        print(f"census: recorded with an error: {census['error']}", file=sys.stderr)
    if args.accounting:
        print("\n".join(accounting_report(entries, multipass, names)))
        return 0
    print("\n".join(rows(entries, names)))
    if args.totals:
        print("\n".join(totals(entries)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
