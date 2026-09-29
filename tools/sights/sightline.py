"""Derive a weapon's sight line from its model, in its fire point's frame.

The frame has its origin at the fire point, x lateral, h up (+Y), and s rearward (+Z), so the barrel
points toward -s. The fire point sits on the bore line at the muzzle.

A scope is found as a run of tube cross-sections above the bore; the sight line runs through the
first and last tube centres. Otherwise the sight line is taken from iron sights: the longest edge of
the upper convex hull of the centreline top profile over the front of the gun, which runs from the
front post tip to the rear sight top.
"""

import math
from dataclasses import dataclass

from weapon import Weapon

# Scope detection.
SLICE = 0.02  # the length of a cross-section slice along s, in metres
RING_TOP_BAND = 0.09  # the vertices within this height of a slice's top form the candidate tube
RING_WIDEST_BAND = 0.0015  # the vertices within this much of the widest |x| mark the tube's axis height
RING_MIN_RADIUS = 0.012
RING_MAX_RADIUS = 0.045
RING_ROUNDNESS = 0.35  # how far the top may sit from one radius above the axis, as a fraction of it
RING_MIN_HEIGHT = 0.03  # a tube's axis sits at least this far above the bore
RING_MIN_VERTICES = 8
RING_SLICE_FLOOR = 0.02  # vertices at or below this height (the barrel and receiver) are not part of a tube
RING_AXIS_TOLERANCE = 0.006  # the largest axis height step between rings of one tube
SCOPE_MIN_LENGTH = 0.10
SCOPE_MIN_RINGS = 3
SCOPE_SEARCH_FRACTION = 0.9  # the scan covers this fraction of the gun behind the muzzle

# Iron-sight profile.
SLAB = 0.004  # the half-width of the centreline slab, in metres
BIN = 0.005  # the profile bin along s, in metres
REAR_FRACTION = 0.75  # the profile covers this fraction of the gun behind the muzzle, excluding the stock

# Plausibility of an iron-sight result.
MAX_PITCH_DEG = 6.0
MIN_SPAN_FRACTION = 0.25  # the sight line's two points span at least this fraction of the gun


@dataclass
class SightLine:
    kind: str  # "scope" or "iron"
    # The two points the line runs through, as (s, h): the front and the rear.
    front: tuple[float, float]
    rear: tuple[float, float]
    length_behind_muzzle: float
    # The line's height at the muzzle (s = 0) and its pitch, positive when the line climbs toward
    # the muzzle.
    height_at_muzzle: float
    pitch_deg: float
    # Why an iron-sight result is implausible, if it is.
    doubt: str | None


def derive(weapon: Weapon) -> SightLine:
    if not weapon.fire_points:
        raise ValueError("the entity has no fire position")
    points = _frame_points(weapon, weapon.fire_points[0].translation)
    if not points:
        raise ValueError("the entity has no model vertices")
    length = max(p[2] for p in points)

    rings = _scope(points, length)
    if rings is not None:
        kind = "scope"
        front, rear = (rings[0][0], rings[0][1]), (rings[-1][0], rings[-1][1])
    else:
        kind = "iron"
        front, rear = _iron(points, length)

    (s_front, h_front), (s_rear, h_rear) = front, rear
    slope = (h_front - h_rear) / (s_rear - s_front)
    pitch = math.degrees(math.atan(slope))
    doubt = None
    if kind == "iron":
        if abs(pitch) > MAX_PITCH_DEG:
            doubt = f"pitch {pitch:.1f} deg exceeds {MAX_PITCH_DEG} deg"
        elif s_rear - s_front < MIN_SPAN_FRACTION * length:
            doubt = f"the sight line spans {s_rear - s_front:.3f} m of a {length:.3f} m gun"
    return SightLine(kind, front, rear, length, h_front + slope * s_front, pitch, doubt)


def _frame_points(weapon: Weapon, origin) -> list[tuple[float, float, float]]:
    ox, oy, oz = origin
    return [(x - ox, y - oy, z - oz) for part in weapon.parts for x, y, z in part.positions]


def _ring(points, s0: float, s1: float):
    """A tube cross-section in [s0, s1): its (axis height, radius), or None."""
    sel = [p for p in points if s0 <= p[2] < s1 and p[1] > RING_SLICE_FLOOR]
    if len(sel) < RING_MIN_VERTICES:
        return None
    top = max(p[1] for p in sel)
    near = [p for p in sel if p[1] > top - RING_TOP_BAND]
    radius = max(abs(p[0]) for p in near)
    widest = [p for p in near if abs(p[0]) > radius - RING_WIDEST_BAND]
    axis = sum(p[1] for p in widest) / len(widest)
    # A tube is widest at its axis height, its top sits one radius above that, and it has vertices
    # on both sides.
    if not RING_MIN_RADIUS <= radius <= RING_MAX_RADIUS:
        return None
    if abs((top - axis) - radius) >= RING_ROUNDNESS * radius:
        return None
    if not min(p[0] for p in widest) < 0 < max(p[0] for p in widest):
        return None
    if axis < RING_MIN_HEIGHT:
        return None
    return axis, radius


def _scope(points, length: float):
    """The longest run of rings with a consistent axis height, as (s, axis height) pairs."""
    rings = []
    s = 0.0
    while s < length * SCOPE_SEARCH_FRACTION:
        ring = _ring(points, s, s + SLICE)
        if ring:
            rings.append((s + SLICE / 2, ring[0]))
        s += SLICE
    if len(rings) < SCOPE_MIN_RINGS:
        return None
    best = None
    for i, first in enumerate(rings):
        run = [first]
        for ring in rings[i + 1 :]:
            if abs(ring[1] - run[-1][1]) < RING_AXIS_TOLERANCE:
                run.append(ring)
        if best is None or run[-1][0] - run[0][0] > best[-1][0] - best[0][0]:
            best = run
    if best[-1][0] - best[0][0] < SCOPE_MIN_LENGTH:
        return None
    return best


def _iron(points, length: float):
    profile = {}
    for x, h, s in points:
        if abs(x) <= SLAB and 0 <= s <= length * REAR_FRACTION:
            b = int(s / BIN)
            if b not in profile or h > profile[b][1]:
                profile[b] = (s, h)
    if len(profile) < 2:
        raise ValueError("the centreline slab has no top profile")
    hull = _upper_hull(profile.values())
    _, front, rear = max((b[0] - a[0], a, b) for a, b in zip(hull, hull[1:]))
    return front, rear


def _upper_hull(points):
    hull = []
    for p in sorted(points):
        while len(hull) >= 2 and _cross(hull[-2], hull[-1], p) >= 0:
            hull.pop()
        hull.append(p)
    return hull


def _cross(o, a, b) -> float:
    return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
