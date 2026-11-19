"""Account for one frame's draw calls under single-pass against multipass.

A single-pass census records each game draw call with how the collapse submitted it. Some of its
calls are the collapse's own: the second eye of a render block re-driven once per eye
(``reissue_eye == 1``) and the second run of a fullscreen pass split per eye (``split_eye == 1``).
Leaving those out gives the game's calls, the draws one walk of the render-pass list makes. A
multipass census of the same scene records both eyes' walks as two dispatches, so its submissions are
what multipass costs for that frame.

The accounting groups both censuses by a key (the render pass, the render-block type, or the
render-block type and vertex shader), and for each group gives the game's calls, the single-pass submissions, the multipass
submissions, and the submissions single-pass would save if every call in the group were drawn once
for both eyes.
"""

from collections import defaultdict
from dataclasses import dataclass
from typing import Callable

ShaderLabel = Callable[[dict], str]


@dataclass
class Account:
    calls: int = 0
    single_pass: int = 0
    multipass: int = 0
    per_eye_extra: int = 0


def game_calls(entry: dict) -> int:
    """The entry's calls the game made, leaving out the collapse's second-eye repeats."""
    if entry.get("reissue_eye") == 1 or entry.get("split_eye") == 1:
        return 0
    return entry["calls"]


def per_eye_extra(entry: dict) -> int:
    """The submissions the entry spends beyond one per game call."""
    if entry["treatment"] == "instance_doubled":
        return 0
    return max(entry["submissions"] - game_calls(entry), 0)


def handling(entry: dict) -> str:
    """How the collapse handles the entry's calls, coarser than the treatment."""
    if entry.get("reissue_eye") is not None:
        return "block_reissue"
    if entry.get("split_eye") is not None:
        return "fullscreen_split"
    return entry["treatment"]


def by_pass(entry: dict, label: ShaderLabel) -> tuple:
    return (entry["pass"], entry["pass_name"])


def by_block(entry: dict, label: ShaderLabel) -> tuple:
    return (entry["block"] or "-",)


def by_family(entry: dict, label: ShaderLabel) -> tuple:
    return (entry["block"] or "-", label(entry["vertex_shader"]))


def by_handling(entry: dict, label: ShaderLabel) -> tuple:
    return (handling(entry),)


def account(single_pass: list[dict], multipass: list[dict] | None, key: Callable,
            label: ShaderLabel) -> dict[tuple, Account]:
    out: dict[tuple, Account] = defaultdict(Account)
    for entry in single_pass:
        row = out[key(entry, label)]
        row.calls += game_calls(entry)
        row.single_pass += entry["submissions"]
        row.per_eye_extra += per_eye_extra(entry)
    for entry in multipass or []:
        if key not in MULTIPASS_KEYS:
            continue
        out[key(entry, label)].multipass += entry["submissions"]
    return dict(out)


def total(accounts: dict[tuple, Account]) -> Account:
    out = Account()
    for row in accounts.values():
        out.calls += row.calls
        out.single_pass += row.single_pass
        out.multipass += row.multipass
        out.per_eye_extra += row.per_eye_extra
    return out


def table(accounts: dict[tuple, Account], heading: str, ranked: bool, with_multipass: bool) -> list[str]:
    """Rows for the accounting, in pass order or ranked by the submissions a single draw would save."""
    order = (sorted(accounts.items(), key=lambda kv: (-kv[1].per_eye_extra, -kv[1].single_pass, kv[0]))
             if ranked else sorted(accounts.items()))
    mp = " multipass" if with_multipass else ""
    lines = [f"{heading}: calls single_pass{mp} per_eye_extra"]
    for key, row in order:
        label = " ".join(str(part) for part in key if not isinstance(part, int))
        mp = f" {row.multipass}" if with_multipass else ""
        lines.append(f"{label} {row.calls} {row.single_pass}{mp} {row.per_eye_extra}")
    whole = total(accounts)
    mp = f" {whole.multipass}" if with_multipass else ""
    lines.append(f"total {whole.calls} {whole.single_pass}{mp} {whole.per_eye_extra}")
    return lines


# A vertex shader is named by the shader object bound, from a registry keyed by pointer, and a
# multipass capture follows a shader reload that can leave those names stale; the render pass and the
# render-block type do not depend on it, so only those keys are compared against multipass.
MULTIPASS_KEYS = (by_pass, by_block)
