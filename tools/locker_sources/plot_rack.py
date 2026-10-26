"""Plot the embedded military rack mesh in its native local coordinates."""

import argparse
import logging
import os
from pathlib import Path
import sys

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

SIGHTS = Path(__file__).resolve().parents[1] / "sights"
sys.path.insert(0, str(SIGHTS))

from archives import GameArchives, read_entity
from rbm import mesh


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("container")
    parser.add_argument("model")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    archives = GameArchives(Path(os.environ["JC3_GAME_DIR"]))
    files = read_entity(archives.read(args.container))
    vertices, triangles = mesh(files[args.model], args.model)
    fig, axes = plt.subplots(1, 3, figsize=(16, 7))
    for axis, (horizontal, vertical, title) in zip(axes, ((0, 1, "Front: X/Y"), (2, 1, "Side: Z/Y"), (0, 2, "Top: X/Z"))):
        axis.scatter([v[horizontal] for v in vertices], [v[vertical] for v in vertices], s=0.2)
        axis.set_title(title)
        axis.set_aspect("equal")
        axis.grid()
    fig.suptitle(args.model)
    fig.tight_layout()
    fig.savefig(args.output, dpi=160)
    logging.info("plotted %d vertices and %d triangles to %s", len(vertices), len(triangles), args.output)


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    main()
