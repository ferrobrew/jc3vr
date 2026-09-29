"""Side views of a weapon's model with its derived sight line, for reviewing the table by eye.

matplotlib is optional: the rest of the tool uses only the standard library, so a missing
matplotlib is reported when a plot is requested, not when the tool starts.
"""

import math
from pathlib import Path

try:
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
except ImportError:
    plt = None

from sightline import SightLine
from weapon import Weapon

# Vertices within this lateral distance of the centreline are drawn in red in the side view: these
# form the iron-sight profile.
CENTRELINE = 0.006


def side_view(weapon: Weapon, line: SightLine, path: Path) -> None:
    if plt is None:
        raise RuntimeError("plots: matplotlib is not installed")
    path.parent.mkdir(parents=True, exist_ok=True)
    fx, fy, fz = weapon.fire_points[0].translation
    fig, (side, top) = plt.subplots(2, 1, figsize=(14, 7), sharex=True)
    for part in weapon.parts:
        colours = ["r" if abs(x) < CENTRELINE else "0.7" for x, _, _ in part.positions]
        side.scatter([z for _, _, z in part.positions], [y for _, y, _ in part.positions], s=1, c=colours)
        top.scatter([z for _, _, z in part.positions], [x for x, _, _ in part.positions], s=1, c="0.5")
    # The bore: the fire point's -Z axis, drawn rearward from the muzzle.
    side.plot([fz, fz + line.length_behind_muzzle], [fy, fy], "b-", lw=0.8)
    side.plot(fz, fy, "bo")
    slope = math.tan(math.radians(line.pitch_deg))
    zs = [fz - 0.3, fz + line.length_behind_muzzle]
    side.plot(zs, [fy + line.height_at_muzzle - slope * (z - fz) for z in zs], "g-", lw=0.8)
    for s, h in (line.front, line.rear):
        side.plot(fz + s, fy + h, "gx", ms=10)
    for axes in (side, top):
        axes.set_aspect("equal")
        axes.grid(True, lw=0.3)
    side.set_title(f"{weapon.name} ({line.kind}): height {line.height_at_muzzle:.4f} m, pitch {line.pitch_deg:.2f} deg")
    fig.savefig(path, dpi=110, bbox_inches="tight")
    plt.close(fig)
