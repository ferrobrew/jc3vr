# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Count the unsafe code in each of the payload's layer crates, to measure the safe wrappers' progress.

Counts, per crate, over its `src/` tree:

  * `unsafe {` blocks,
  * `unsafe fn` declarations (including `unsafe extern "..." fn`),
  * `unsafe impl` items,
  * direct fetches of the local player (`GetLocalPlayerCharacter`), which the body crate's
    `LocalPlayer` handle replaces,
  * reaches of the handle (`LocalPlayer::get`).

These are line-grep counts, not AST counts: a block or signature split across lines is counted once,
and matches inside comments and strings are counted too. Compare runs of the same script, not the
numbers against another tool's.

Usage: `uv run tools/analysis/unsafe_counts.py [--repo PATH]`
"""

import argparse
import re
from pathlib import Path

PATTERNS = {
    "unsafe {": re.compile(r"unsafe \{"),
    "unsafe fn": re.compile(r'unsafe (?:extern "[^"]*" )?fn '),
    "unsafe impl": re.compile(r"unsafe impl\b"),
    "GetLocalPlayerCharacter": re.compile(r"GetLocalPlayerCharacter"),
    "LocalPlayer::get": re.compile(r"LocalPlayer::get\("),
}


def crate_dirs(repo: Path) -> list[Path]:
    dirs = sorted(p for p in (repo / "crates").glob("jc3vr-*") if (p / "src").is_dir())
    payload = repo / "payload"
    if (payload / "src").is_dir():
        dirs.append(payload)
    return dirs


def count(crate: Path) -> dict[str, int]:
    totals = dict.fromkeys(PATTERNS, 0)
    for path in (crate / "src").rglob("*.rs"):
        text = path.read_text(encoding="utf-8")
        for name, pattern in PATTERNS.items():
            totals[name] += len(pattern.findall(text))
    return totals


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()

    names = list(PATTERNS)
    widths = [max(len(n), 6) for n in names]
    print(f"{'crate':<16} " + " ".join(f"{n:>{w}}" for n, w in zip(names, widths)))
    grand = dict.fromkeys(names, 0)
    for crate in crate_dirs(args.repo):
        totals = count(crate)
        for n in names:
            grand[n] += totals[n]
        print(f"{crate.name:<16} " + " ".join(f"{totals[n]:>{w}}" for n, w in zip(names, widths)))
    print(f"{'total':<16} " + " ".join(f"{grand[n]:>{w}}" for n, w in zip(names, widths)))


if __name__ == "__main__":
    main()
