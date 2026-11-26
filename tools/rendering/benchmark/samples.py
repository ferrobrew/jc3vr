"""Where the draw task's time goes, from a benchmark run's draw-task samples
(`crates/jc3vr-render/src/testing/benchmark/sampler.rs`).

Each sample is one instant of the thread running the draw task: the render pass and render-block
type the draw path had published, whether it was inside a per-eye re-issue, the instruction pointer,
and the return addresses found on its stack. This attributes each sample to the module its
instruction pointer lies in (the game, the Direct3D runtime, the payload, Wine), and the samples
outside the game's own code to the innermost game address that called out, so the tables read as
the share of the draw task's wall time, and with the run's median draw task as milliseconds.

`python3 -m tools.rendering benchmark samples <run.json> [...]` prints the tables; several runs of
the same mode pool their samples."""

import argparse
import json
import re
import statistics
import subprocess
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

PASS_ENUM = Path("jc3gi/src/graphics_engine/render_engine.rs")
NO_PASS = 255
# The modules a sample's instruction pointer is grouped under; anything else is "other".
MODULE_GROUPS = (
    ("game", ("justcause3.exe",)),
    ("d3d (DXVK)", ("d3d11.dll", "dxgi.dll", "d3d10core.dll")),
    ("payload", ("jc3vr_payload",)),
    ("wine ntdll", ("ntdll.dll",)),
    ("wine kernel", ("kernelbase.dll", "kernel32.dll", "win32u.dll")),
    ("c runtime", ("ucrtbase.dll", "msvcr", "msvcp", "vcruntime")),
)


@dataclass(frozen=True)
class Module:
    name: str
    base: int
    size: int

    def contains(self, address: int) -> bool:
        return self.base <= address < self.base + self.size


@dataclass
class Pool:
    modules: list[Module]
    block_types: dict[int, str]
    rows: list[list[int]]
    draw_ms: float
    runs: int
    idle: int
    torn: int
    suspend_us: float


def pass_names(root: Path = Path(".")) -> dict[int, str]:
    """The render-pass names by id, from the generated bindings."""
    text = (root / PASS_ENUM).read_text()
    body = text[text.index("pub enum RenderPassId"):]
    body = body[:body.index("}")]
    return {int(value): name for name, value in re.findall(r"(\w+) = (\d+)isize", body)}


def load(paths: list[Path]) -> Pool:
    pool = None
    draw = []
    for path in paths:
        run = json.loads(path.read_text())
        samples = run["payload"].get("draw_task_samples")
        if samples is None:
            raise ValueError(f"benchmark samples: {path} has no draw-task samples")
        columns = run["payload"]["columns"]
        frames = run["payload"]["frames"]
        draw_index = columns.index("draw_task_us")
        draw += [row[draw_index] / 1000 for row in frames]
        modules = [Module(m["name"], m["base"], m["size"]) for m in samples["modules"]]
        types = {address: name for address, name in samples["block_types"]}
        if pool is None:
            pool = Pool(modules, types, [], 0.0, 0, 0, 0, 0.0)
        pool.block_types.update(types)
        pool.rows += samples["rows"]
        pool.runs += 1
        pool.idle += samples["idle"]
        pool.torn += samples["torn"]
        pool.suspend_us += samples["mean_suspend_us"]
    if pool is None:
        raise ValueError("benchmark samples: no runs given")
    pool.draw_ms = statistics.median(draw) if draw else 0.0
    pool.suspend_us /= pool.runs
    return pool


def module_of(modules: list[Module], address: int) -> Module | None:
    for module in modules:
        if module.contains(address):
            return module
    return None


def group_of(module: Module | None) -> str:
    if module is None:
        return "unmapped (wine unix side)"
    name = module.name.lower()
    for group, prefixes in MODULE_GROUPS:
        if any(name.startswith(prefix) for prefix in prefixes):
            return group
    return f"other ({module.name})"


def stage_of(pass_id: int, names: dict[int, str]) -> str:
    """The engine's seam the pass belongs to, by the pass ranges the engine draws."""
    if pass_id == NO_PASS:
        return "outside a pass"
    z_occluders = next(k for k, v in names.items() if v == "RP_Z_OCCLUDERS")
    last_gbuffer = next(k for k, v in names.items() if v == "RP_LAST_GBUFFER")
    water = next(k for k, v in names.items() if v == "RP_REFLECTIVE_WATER_PLANES")
    post = next(k for k, v in names.items() if v == "RP_POSTEFFECTS")
    if pass_id < z_occluders:
        return "PreDraw (prepasses)"
    if pass_id < last_gbuffer:
        return "DrawGBuffer"
    if water <= pass_id < post:
        return "Draw (scene)"
    if pass_id >= post:
        return "DrawPosteffects"
    return "between ranges"


def table(title: str, counts: Counter, total: int, draw_ms: float, limit: int = 30) -> list[str]:
    lines = [f"### {title}", "", "| | samples | share | ms per frame |", "|---|---|---|---|"]
    for key, count in counts.most_common(limit):
        share = count / total if total else 0.0
        lines.append(f"| {key} | {count} | {share:.1%} | {share * draw_ms:.2f} |")
    rest = sum(counts.values()) - sum(c for _, c in counts.most_common(limit))
    if rest:
        lines.append(f"| (the rest) | {rest} | {rest / total:.1%} | {rest / total * draw_ms:.2f} |")
    return lines + [""]


def report(pool: Pool, names: dict[int, str], game_symbols: dict[int, str] | None = None) -> str:
    total = len(pool.rows)
    game = next((m for m in pool.modules if m.name.lower() == "justcause3.exe"), None)
    lines = [f"{total} samples over {pool.runs} runs; {pool.idle} wake-ups found no draw task, "
             f"{pool.torn} samples were torn; the median draw task {pool.draw_ms:.2f} ms; a sample "
             f"suspended the draw thread for {pool.suspend_us:.0f} us on average.", ""]
    by_module = Counter(group_of(module_of(pool.modules, row[3])) for row in pool.rows)
    lines += table("By module of the instruction pointer", by_module, total, pool.draw_ms)
    by_stage = Counter(stage_of(row[0], names) for row in pool.rows)
    lines += table("By engine seam", by_stage, total, pool.draw_ms)
    by_pass = Counter(names.get(row[0], str(row[0])) if row[0] != NO_PASS else "outside a pass"
                      for row in pool.rows)
    lines += table("By render pass", by_pass, total, pool.draw_ms, limit=40)
    by_type = Counter((pool.block_types.get(row[2], "none") if row[0] != NO_PASS else "outside a pass")
                      for row in pool.rows)
    lines += table("By render-block type", by_type, total, pool.draw_ms)
    by_reissue = Counter({0: "not in a re-issue", 1: "re-issue, eye 0", 2: "re-issue, eye 1"}
                         .get(row[1], str(row[1])) for row in pool.rows)
    lines += table("By per-eye re-issue", by_reissue, total, pool.draw_ms)
    by_type_module = Counter(
        f"{pool.block_types.get(row[2], 'none') if row[0] != NO_PASS else 'outside a pass'} / "
        f"{group_of(module_of(pool.modules, row[3]))}" for row in pool.rows)
    lines += table("By render-block type and module", by_type_module, total, pool.draw_ms, limit=40)
    owners = Counter()
    for row in pool.rows:
        address = first_caller(pool, row)
        owner = "none found" if address is None else group_of(module_of(pool.modules, address))
        owners[f"{owner} -> {group_of(module_of(pool.modules, row[3]))}"] += 1
    lines += table("Whose code it is: the innermost frame outside the runtimes, and where the "
                   "instruction pointer was", owners, total, pool.draw_ms)
    if game is not None:
        callers = Counter()
        for row in pool.rows:
            address = first_caller(pool, row)
            if address is None or not game.contains(address):
                continue
            label = (game_symbols or {}).get(address) or f"{address:#x}"
            callers[f"{label} -> {group_of(module_of(pool.modules, row[3]))}"] += 1
        lines += table("The game's code running, or calling into a runtime", callers, total,
                       pool.draw_ms, limit=60)
    return "\n".join(lines)


def payload_symbols(pool: Pool, dll: Path) -> dict[int, str]:
    """Each payload address the samples name (instruction pointers and the innermost payload return
    address), named by `llvm-symbolizer` from the payload DLL's PDB as the outermost function its
    inlined frames sit in, with the innermost inlined one beside it."""
    payload = next((m for m in pool.modules if m.name.lower().startswith("jc3vr_payload")), None)
    if payload is None:
        return {}
    addresses = {address for row in pool.rows
                 if (address := first_caller(pool, row)) and payload.contains(address)}
    ordered = sorted(addresses)
    if not ordered:
        return {}
    output = subprocess.run(
        ["llvm-symbolizer", f"--obj={dll}", "--relative-address", "--inlines",
         "--output-style=JSON"],
        input="\n".join(f"{a - payload.base:#x}" for a in ordered), capture_output=True, text=True,
        check=True).stdout
    names = {}
    for address, line in zip(ordered, output.splitlines()):
        frames = json.loads(line).get("Symbol") or []
        if not frames:
            continue
        outer, inner = frames[-1]["FunctionName"], frames[0]["FunctionName"]
        names[address] = outer if outer == inner else f"{outer} [{inner}]"
    return names


RUNTIME_GROUPS = ("d3d (DXVK)", "wine ntdll", "wine kernel", "c runtime",
                  "unmapped (wine unix side)")


def first_caller(pool: Pool, row: list[int]) -> int | None:
    """The innermost address on the sample that is neither the Direct3D runtime nor Wine nor the C
    runtime: the code that is running, or that called into those runtimes."""
    for address in row[3:]:
        if address and group_of(module_of(pool.modules, address)) not in RUNTIME_GROUPS:
            return address
    return None


def payload_report(pool: Pool, names: dict[int, str]) -> list[str]:
    total = len(pool.rows)
    payload = next(m for m in pool.modules if m.name.lower().startswith("jc3vr_payload"))
    counts = Counter()
    for row in pool.rows:
        address = first_caller(pool, row)
        if address is None or not payload.contains(address):
            continue
        where = group_of(module_of(pool.modules, row[3]))
        counts[f"{shorten(names.get(address, hex(address)))} -> {where}"] += 1
    return table("Payload code running, or calling into a runtime (the innermost frame outside the "
                 "runtimes is the payload's)", counts, total, pool.draw_ms, limit=50)


def shorten(name: str) -> str:
    """A Rust symbol without its generic arguments and hash, to keep the table readable."""
    name = re.sub(r"::h[0-9a-f]{16}$", "", name)
    depth, out = 0, []
    for char in name:
        if char == "<":
            depth += 1
        elif char == ">":
            depth -= 1
        elif depth == 0:
            out.append(char)
    return "".join(out)[:160]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="benchmark samples")
    parser.add_argument("runs", nargs="+", type=Path)
    parser.add_argument("--payload-dll", type=Path,
                        default=Path("target/x86_64-pc-windows-msvc/release/jc3vr_payload_loaded.dll"),
                        help="the payload DLL the runs loaded, to name the payload's functions")
    parser.add_argument("--symbols", type=Path,
                        help="a JSON object mapping game addresses (hex) to names, to label them")
    parser.add_argument("--addresses", type=Path,
                        help="write the distinct game addresses, most sampled first, to this file")
    args = parser.parse_args(argv)
    pool = load(args.runs)
    symbols = None
    if args.symbols:
        symbols = {int(k, 16): v for k, v in json.loads(args.symbols.read_text()).items()}
    if args.addresses:
        game = next(m for m in pool.modules if m.name.lower() == "justcause3.exe")
        counts = Counter(address for row in pool.rows
                         if (address := first_caller(pool, row)) and game.contains(address))
        args.addresses.write_text("\n".join(f"{a:#x} {c}" for a, c in counts.most_common()) + "\n")
    print(report(pool, pass_names(), symbols))
    if args.payload_dll.exists():
        print("\n".join(payload_report(pool, payload_symbols(pool, args.payload_dll))))
    return 0
