"""Side, top, and front views of a weapon's model with its derived well, for reviewing the table by
eye.

matplotlib is optional, as for ``plot.py``: a missing matplotlib is reported when a plot is
requested, not when the tool starts.
"""

from pathlib import Path

try:
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from matplotlib.collections import PolyCollection
except ImportError:
    plt = None

from weapon import Weapon
from wells import Well

AVAILABLE = plt is not None
# The length of the drawn insertion axis, in metres.
AXIS_LENGTH = 0.08
# The views: their titles and the weapon-space axes drawn horizontally and vertically.
VIEWS = (("side (z, y)", 2, 1), ("top (z, x)", 2, 0), ("front (x, y)", 0, 1))


def views(weapon: Weapon, well: Well, path: Path) -> None:
    if plt is None:
        raise RuntimeError("plots: matplotlib is not installed")
    path.parent.mkdir(parents=True, exist_ok=True)
    fig, axes = plt.subplots(1, 3, figsize=(24, 6), gridspec_kw={"width_ratios": [3, 3, 1]})
    for (title, h, v), ax in zip(VIEWS, axes):
        for part in weapon.parts:
            colour = "tab:red" if part is well.part else "0.35"
            polys = [[(part.positions[i][h], part.positions[i][v]) for i in tri] for tri in part.triangles]
            ax.add_collection(PolyCollection(polys, facecolors=colour, edgecolors="none", alpha=0.12))
        if weapon.fire_points:
            fp = weapon.fire_points[0].translation
            ax.plot(fp[h], fp[v], "b+", ms=10)
        if well.mouth is not None:
            ax.plot(well.mouth[h], well.mouth[v], "go", ms=8, mfc="none", mew=2)
            if well.axis is not None:
                # The axis is drawn ending at the mouth, along the direction of insertion.
                start = [well.mouth[i] - AXIS_LENGTH * well.axis[i] for i in range(3)]
                ax.annotate("", xy=(well.mouth[h], well.mouth[v]), xytext=(start[h], start[v]), arrowprops={"arrowstyle": "->", "color": "g", "lw": 1.5})
        if well.handle is not None:
            ax.plot(well.handle[h], well.handle[v], "mx", ms=10, mew=2)
        ax.set_aspect("equal")
        ax.autoscale()
        ax.minorticks_on()
        ax.grid(True, which="major", lw=0.3)
        ax.grid(True, which="minor", lw=0.1)
        ax.set_title(title)
    doubt = f"; doubtful: {', '.join(well.doubts)}" if well.doubts else ""
    axes[0].set_title(f"{weapon.name}: {well.kind}/{well.grade}, action {well.action} (green: mouth and axis, magenta: handle){doubt}", loc="left", fontsize=9)
    fig.savefig(path, dpi=90, bbox_inches="tight")
    plt.close(fig)
