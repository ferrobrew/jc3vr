"""Pick a borrowed spare magazine for a gun whose magazine is modelled into its body with no prop.

The Automat 12's box magazine is part of its body: it has no magazine part and no spare magazine
prop, so nothing shows in the hand that loads it and nothing drops when it is ejected. Another hand
weapon's spare magazine prop stands in for it. The donor is the one whose prop's model best matches
the built-in magazine in size: the magazine's extents across the gun, along its insertion axis, and
along the barrel, against the prop model's, compared as the sum of their differences. Only a prop
whose own insertion axis is the gun's (so it needs no turn to line up) and which depicts the
magazine a hand loads (``spare_top_m`` is set for its weapon) is a candidate.

The built-in magazine is the body's vertices in the column above the well's mouth, which is the
magazine's base, up to its visible length, leaving out the top fifth, where the column meets the
receiver.
"""

from dataclasses import dataclass

from weapon import Vec3, Weapon

# The column's lateral reach from the mouth: wide enough for any box magazine, narrow enough to
# leave out the grip and the stock.
COLUMN_REACH = 0.06
# The top of the column, as a share of its visible length, which meets the receiver.
COLUMN_TOP_SHARE = 0.8
# The most a candidate's insertion axis may differ from the gun's, as a dot product.
AXIS_AGREEMENT = 0.99


@dataclass
class Donor:
    # The donor weapon's entity name, and its spare magazine's spawn tag.
    weapon: str
    tag: str
    # The prop model's points, in its own frame.
    positions: list[Vec3]
    # The direction the magazine goes in, in the prop model's frame.
    axis: Vec3


@dataclass
class Borrowed:
    donor: Donor
    # The sum of the extents' differences, metres.
    mismatch: float
    # Where the prop model's origin sits in the gun's frame with the magazine seated: the centre
    # of its base on the mouth, which is centred on the column.
    origin: Vec3


def built_in_extents(gun: Weapon, mouth: Vec3, axis: Vec3, length: float) -> Vec3 | None:
    """The built-in magazine's extents across the gun, along ``axis``, and along the barrel, or
    ``None`` when the column has no vertices. ``axis`` is close to +Y, as it is for a magazine
    hanging below the receiver."""
    column = [
        p
        for part in gun.parts
        for p in part.positions
        if 0.0 <= _dot(_sub(p, mouth), axis) <= COLUMN_TOP_SHARE * length
        and abs(p[0] - mouth[0]) <= COLUMN_REACH
        and abs(p[2] - mouth[2]) <= COLUMN_REACH
    ]
    if not column:
        return None
    return (_span(column, 0), length, _span(column, 2))


def choose(extents: Vec3, axis: Vec3, mouth: Vec3, donors: list[Donor]) -> Borrowed | None:
    """The donor whose prop best matches ``extents``, seated with its base on ``mouth``."""
    best = None
    for donor in donors:
        if _dot(donor.axis, axis) < AXIS_AGREEMENT:
            continue
        mismatch = sum(abs(a - b) for a, b in zip(extents, _prop_extents(donor)))
        if best is None or mismatch < best.mismatch:
            best = Borrowed(donor, mismatch, _origin(donor, mouth))
    return best


def _prop_extents(donor: Donor) -> Vec3:
    # The prop's frame has the gun's axes once the axes agree, so its extents compare directly.
    return (_span(donor.positions, 0), _span(donor.positions, 1), _span(donor.positions, 2))


def _origin(donor: Donor, mouth: Vec3) -> Vec3:
    ps = donor.positions
    centre = tuple((min(p[i] for p in ps) + max(p[i] for p in ps)) / 2 for i in range(3))
    bottom = min(_dot(p, donor.axis) for p in ps)
    shift = bottom - _dot(centre, donor.axis)
    base = tuple(c + a * shift for c, a in zip(centre, donor.axis))
    return _sub(mouth, base)


def _span(points: list[Vec3], i: int) -> float:
    return max(p[i] for p in points) - min(p[i] for p in points)


def _sub(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _dot(a: Vec3, b: Vec3) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
