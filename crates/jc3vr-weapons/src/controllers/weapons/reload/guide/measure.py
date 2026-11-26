import json
import logging
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[6]
sys.path.insert(0, str(ROOT / "tools/sights"))
from archives import GameArchives, read_entity
from extract_sights import _player_weapon_entities, FILE_LISTS
import rbm
import spares
import weapon
import wells

logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stderr)
arc = GameArchives(Path(os.environ["JC3_GAME_DIR"]))
props = spares.SpareMagazines(arc)
table = json.load(open(ROOT / "tools/sights/wells.json"))
paths = {
    weapon.name_hash(p): p
    for p in _player_weapon_entities(
        Path(os.environ["JC3_GIBBED_TOOLS"]) / FILE_LISTS, arc
    )
}
loaded = {
    e["weapon"]: weapon.load(paths[e["weapon_hash"]], arc, skipped_part_names=())
    for e in table
    if e["kind"] != "none"
}


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def cross(a, b):
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def hull(points):
    ps = sorted(set((round(x, 5), round(y, 5)) for x, y in points))

    def half(ps):
        out = []
        for p in ps:
            while (
                len(out) > 1
                and (out[-1][0] - out[-2][0]) * (p[1] - out[-2][1])
                - (out[-1][1] - out[-2][1]) * (p[0] - out[-2][0])
                <= 0
            ):
                out.pop()
            out.append(p)
        return out

    return half(ps)[:-1] + half(ps[::-1])[:-1]


out = []
for e in table:
    if e["kind"] == "none":
        continue
    w = loaded[e["weapon"]]
    axis = e["insert_axis"]
    source = None
    if e["round_model"]:
        files = read_entity(arc.read(paths[e["weapon_hash"]]))
        closest = files[e["round_model"]].decode().split()[0]
        points = rbm.positions(files.get(closest) or arc.read(closest), closest)
        old = e["round_axis"]
        top = e["round_top_m"]
        v = cross(old, axis)
        c = dot(old, axis)

        def rotate(p):
            if c < -0.999:
                return (-p[0], p[1], -p[2])
            a = cross(v, p)
            b = cross(v, a)
            return tuple(p[i] + a[i] + b[i] / (1 + c) for i in range(3))

        points = [rotate(sub(p, top)) for p in points]
        source = e["round_model"]
    else:
        part = next(
            (p for n in wells.ROUND_PARTS for p in w.parts if p.name == n), None
        )
        donor = (
            loaded.get(e["spare_from"].removesuffix("_magazine"))
            if e["spare_from"]
            else None
        )
        if e["weapon"] == "w013_pistol_balloon_head":
            original = loaded["w011_pistol_u_pozhar_98"]
            assert w.parts[0].positions == original.parts[0].positions
            donor = original
        spare = props.model(donor or w)
        if e["spare_top_m"] or donor:
            assert spare
            points = spare.positions
            if part and part.matrix:
                m = part.matrix
                points = [
                    tuple(sum(p[i] * m[i * 4 + j] for i in range(3)) for j in range(3))
                    for p in points
                ]
            source = spare.model
        else:
            assert part
            points = part.positions
            source = part.model
        top = spares.top_centre(points, axis)
        points = [sub(p, top) for p in points]
    segments = []
    centre = [
        (min(p[i] for p in points) + max(p[i] for p in points)) / 2 for i in range(3)
    ]
    for i, j in [(0, 1), (0, 2), (1, 2)]:
        ring = hull((p[i], p[j]) for p in points)

        def point(p):
            q = centre.copy()
            q[i], q[j] = p
            return [round(x, 5) + 0.0 for x in q]

        segments += [[point(a), point(b)] for a, b in zip(ring, ring[1:] + ring[:1])]
    out.append(
        dict(
            weapon_hash=e["weapon_hash"],
            kind=e["kind"],
            axis=axis,
            source=source,
            segments=segments,
        )
    )
    logging.info("%s %s %d %s", e["weapon"], e["kind"], len(segments), source)
Path(__file__).with_name("measured.json").write_text(
    "[\n" + ",\n".join(json.dumps(e, separators=(",", ":")) for e in out) + "\n]\n"
)
