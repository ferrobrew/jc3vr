"""The geometry of a door handle, pure: where an entry clip's hand is when the door opens, which
door it is on, the point on the door's outer surface under it, and the handle a door's box and
hinge give when no clip reaches it.

Entry clips. A character entering a vehicle plays an entry clip whose root motion the engine warps
(``NInteractionGraphEntryTasks::NWarpedMotion``) so that the entry's alignment bone ends the clip
on the entry's destination node: the warp corrects the horizontal position, the height, and the
heading, each over its own segment of the clip (``SEG_WARP_HORIZONTAL``, ``SEG_WARP_VERTICAL``,
``SEG_WARP_ORIENTATION``, or ``SEG_WARP`` for all three), and before a segment starts that part of
the motion is the clip's own, from where the character stood. So at a time ``t`` after the
horizontal and heading segments, the character is where the destination puts it less the motion
still to come: ``bone(t) * inverse(anchor(end)) * destination``. Its height blends between that and
the clip's own height above the ground the character started on, by how far the vertical segment
has run.
"""

import math
from dataclasses import dataclass

from xform import Vec3, Xform, add, cross, dot, length, scale, sub

HANDS = (("left", "LeftHandAttach"), ("right", "RightHandAttach"))


@dataclass(frozen=True)
class WarpProgress:
    horizontal: float
    vertical: float
    orientation: float
    """How far each warp segment has run at a time, ``0..=1``."""

    @property
    def complete_in_plane(self) -> bool:
        return self.horizontal >= 1.0 and self.orientation >= 1.0


def warp_progress(segments: dict[str, tuple[float, float]], t: float) -> WarpProgress | None:
    """How far the warp has run at ``t``, from the clip's segments; ``None`` for a clip with no warp
    segment."""

    def fraction(*names: str) -> float | None:
        for name in names:
            if name in segments:
                start, end = segments[name]
                if end <= start:
                    return 1.0 if t >= end else 0.0
                return min(max((t - start) / (end - start), 0.0), 1.0)
        return None

    h = fraction("SEG_WARP_HORIZONTAL", "SEG_WARP")
    v = fraction("SEG_WARP_VERTICAL", "SEG_WARP")
    o = fraction("SEG_WARP_ORIENTATION", "SEG_WARP")
    if h is None and v is None and o is None:
        return None
    # A part of the warp without a segment of its own is not warped: it ends where the clip does.
    return WarpProgress(1.0 if h is None else h, 1.0 if v is None else v, 1.0 if o is None else o)


def bone_in_vehicle(bone_at_t: Xform, anchor_at_end: Xform, destination: Xform, progress: WarpProgress, ground_y: float | None) -> Vec3:
    """Where a bone at ``bone_at_t`` (in the space of the character's start) is in the vehicle's
    space, for a clip whose alignment bone ends at ``anchor_at_end`` and is warped onto
    ``destination``."""
    landed = bone_at_t.then(anchor_at_end.inverse()).then(destination).origin
    if ground_y is None or progress.vertical >= 1.0:
        return landed
    unwarped_y = ground_y + bone_at_t.origin[1]
    y = progress.vertical * landed[1] + (1.0 - progress.vertical) * unwarped_y
    return (landed[0], y, landed[2])


@dataclass(frozen=True)
class Box:
    low: Vec3
    high: Vec3

    def distance(self, p: Vec3) -> float:
        nearest = tuple(min(max(p[i], self.low[i]), self.high[i]) for i in range(3))
        return length(sub(p, nearest))

    @property
    def centre(self) -> Vec3:
        return scale(add(self.low, self.high), 0.5)

    @property
    def size(self) -> Vec3:
        return sub(self.high, self.low)


@dataclass(frozen=True)
class SurfacePoint:
    point: Vec3
    normal: Vec3
    on_mesh: bool


def surface_point(p: Vec3, outward: Vec3, box: Box, triangles: list) -> SurfacePoint:
    """The point of the door's outer surface under ``p``, for a door whose outside is along
    ``outward`` (a unit axis in the door's space): the outermost crossing of the mesh by the line
    through ``p`` along ``outward``; when the line misses the mesh (above a door's skin, beside
    its edge), the nearest point of a triangle of the outer skin that faces along the axis; and without triangles, the
    box's outer face."""
    best = None
    for a, b, c in triangles:
        hit = _line_triangle(p, outward, a, b, c)
        if hit is not None and (best is None or hit[0] > best[0]):
            best = hit
    if best is not None:
        t, normal = best
        if dot(normal, outward) < 0:
            normal = scale(normal, -1.0)
        return SurfacePoint(add(p, scale(outward, t)), normal, True)
    axis = max(range(3), key=lambda i: abs(outward[i]))
    outer_face = box.high[axis] if outward[axis] > 0 else box.low[axis]
    nearest = None
    for a, b, c in triangles:
        # Only the skin near the outer face: a door's inner trim and window frame face the same
        # way and lie farther in.
        if min(abs(v[axis] - outer_face) for v in (a, b, c)) > SKIN_DEPTH_M:
            continue
        n = cross(sub(b, a), sub(c, a))
        nl = length(n)
        # The winding is not relied on: a face across the outward axis counts either way.
        if nl <= 0 or abs(dot(n, outward)) / nl < OUTWARD_FACING:
            continue
        q = _closest_on_triangle(p, a, b, c)
        d = length(sub(q, p))
        if nearest is None or d < nearest[0]:
            n = scale(n, 1.0 / nl)
            nearest = (d, q, n if dot(n, outward) >= 0 else scale(n, -1.0))
    if nearest is not None:
        return SurfacePoint(nearest[1], nearest[2], True)
    q = [min(max(p[i], box.low[i]), box.high[i]) for i in range(3)]
    q[axis] = box.high[axis] if outward[axis] > 0 else box.low[axis]
    normal = [0.0, 0.0, 0.0]
    normal[axis] = 1.0 if outward[axis] > 0 else -1.0
    return SurfacePoint(tuple(q), tuple(normal), False)


# How far in from the box's outer face a door's outer skin can lie, metres: the curve of a body
# side above and below its widest line.
SKIN_DEPTH_M = 0.1
# How closely a triangle's normal must follow the door's outward axis, either way, to count as
# skin.
OUTWARD_FACING = 0.7


def _closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3:
    """The point of the triangle nearest ``p`` (Ericson, Real-Time Collision Detection, 5.1.5)."""
    ab, ac, ap = sub(b, a), sub(c, a), sub(p, a)
    d1, d2 = dot(ab, ap), dot(ac, ap)
    if d1 <= 0 and d2 <= 0:
        return a
    bp = sub(p, b)
    d3, d4 = dot(ab, bp), dot(ac, bp)
    if d3 >= 0 and d4 <= d3:
        return b
    vc = d1 * d4 - d3 * d2
    if vc <= 0 and d1 >= 0 and d3 <= 0:
        return add(a, scale(ab, d1 / (d1 - d3)))
    cp = sub(p, c)
    d5, d6 = dot(ab, cp), dot(ac, cp)
    if d6 >= 0 and d5 <= d6:
        return c
    vb = d5 * d2 - d1 * d6
    if vb <= 0 and d2 >= 0 and d6 <= 0:
        return add(a, scale(ac, d2 / (d2 - d6)))
    va = d3 * d6 - d5 * d4
    if va <= 0 and (d4 - d3) >= 0 and (d5 - d6) >= 0:
        return add(b, scale(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))))
    denom = 1.0 / (va + vb + vc)
    return add(a, add(scale(ab, vb * denom), scale(ac, vc * denom)))


def _line_triangle(origin: Vec3, direction: Vec3, a: Vec3, b: Vec3, c: Vec3):
    """Where the infinite line ``origin + t * direction`` crosses the triangle: ``(t, normal)``."""
    e1 = sub(b, a)
    e2 = sub(c, a)
    h = cross(direction, e2)
    det = dot(e1, h)
    if abs(det) < 1e-12:
        return None
    inv = 1.0 / det
    s = sub(origin, a)
    u = dot(s, h) * inv
    if u < 0.0 or u > 1.0:
        return None
    q = cross(s, e1)
    v = dot(direction, q) * inv
    if v < 0.0 or u + v > 1.0:
        return None
    t = dot(e2, q) * inv
    n = cross(e1, e2)
    nl = length(n)
    if nl <= 0:
        return None
    return t, scale(n, 1.0 / nl)


SIDE, FRONT, REAR = "side", "front", "rear"

# The door slots (``NVehicle::EDoorSlot``) in the engine's order, by the behaviour name of the
# parts that fill them.
DOOR_SLOTS = (
    "FrontLeftDoor",
    "FrontRightDoor",
    "RearLeftDoor",
    "RearRightDoor",
    "CenterLeftDoor",
    "CenterRightDoor",
    "TurretDoor",
    "Trunk",
    "Hood",
    "RearDoor",
    "CargoBayDoor",
    "FoldingWing",
    "MissileHatch",
)


def kind_of(behaviour: str) -> str:
    """How a door opens, from its part's behaviour (``FrontLeftDoor``, ``Hood``, ``Trunk``, ...),
    matching the payload's reading of the door slots."""
    b = behaviour.lower()
    if "hood" in b:
        return FRONT
    if any(k in b for k in ("trunk", "reardoor", "cargo", "hatch")):
        return REAR
    return SIDE


def outward_axis(door_box: Box, to_vehicle: Xform, kind: str) -> tuple[int, Vec3]:
    """The door's thinnest box axis and the unit direction along it that points out of the
    vehicle, in the door's space."""
    from_vehicle = to_vehicle.inverse()
    size = door_box.size
    thickness = min(range(3), key=lambda i: size[i])
    out = sub(door_box.centre, from_vehicle.apply((0.0, 0.0, 0.0)))
    axis = [0.0, 0.0, 0.0]
    axis[thickness] = 1.0 if out[thickness] >= 0 else -1.0
    return thickness, tuple(axis)


HANDLE_INSET = 0.1


def hinge_handle(door_box: Box, to_vehicle: Xform, kind: str, hinge: Vec3 | None, height: float | None) -> tuple[Vec3, int, Vec3, str]:
    """A handle from the door's box: on the outer face, at the edge across from the hinge's, a
    tenth of the door's run in from it, and halfway along that edge, or at ``height`` (vehicle
    space) for a side door when one is given. The hinge's edge is the one nearer the door's
    constraint point ``hinge`` (door space), or the kind's own: a side door's front, a hood's back,
    a trunk's top. Returns the point (door space), the thickness axis, the outward axis, and which
    rule placed the hinge."""
    from_vehicle = to_vehicle.inverse()
    size = door_box.size
    centre = door_box.centre
    thickness, outward = outward_axis(door_box, to_vehicle, kind)
    up = from_vehicle.rotate((0.0, 1.0, 0.0))
    side = from_vehicle.rotate((1.0, 0.0, 0.0))
    if kind == SIDE:
        across = _axis_along(up, (thickness,))
    else:
        across = _axis_along(side, (thickness,))
    along = next(i for i in range(3) if i not in (thickness, across))
    run = max(size[along], 1e-4)
    if hinge is not None:
        hinge_at_low = (hinge[along] - door_box.low[along]) / run < 0.5
        rule = "constraint"
    else:
        toward = {
            SIDE: from_vehicle.rotate((0.0, 0.0, -1.0)),
            FRONT: from_vehicle.rotate((0.0, 0.0, 1.0)),
            REAR: add(up, from_vehicle.rotate((0.0, 0.0, -1.0))),
        }[kind]
        hinge_at_low = toward[along] < 0.0
        rule = "kind"
    point = list(centre)
    point[along] = door_box.high[along] - run * HANDLE_INSET if hinge_at_low else door_box.low[along] + run * HANDLE_INSET
    if kind == SIDE and height is not None:
        # The height is a vehicle-space y; move along the door's up axis to meet it.
        current = to_vehicle.apply(tuple(point))[1]
        up_axis = _axis_along(up, (thickness,))
        point[up_axis] = min(max(point[up_axis] + (height - current), door_box.low[up_axis]), door_box.high[up_axis])
    point[thickness] = door_box.high[thickness] if outward[thickness] > 0 else door_box.low[thickness]
    return tuple(point), thickness, outward, rule


def _axis_along(direction: Vec3, taken: tuple[int, ...]) -> int:
    return max((i for i in range(3) if i not in taken), key=lambda i: abs(direction[i]))


def self_check() -> list[str]:
    """Check the pure geometry on made-up doors; returns the failures."""
    failures = []

    def check(name: str, ok: bool):
        if not ok:
            failures.append(name)

    # A clip whose anchor ends a metre ahead of where it started, warped onto a destination two
    # metres to the side: a bone half-way is placed relative to the destination.
    anchor_end = Xform(origin=(0.0, 0.3, 1.0))
    destination = Xform(origin=(2.0, 0.5, 0.0))
    bone = Xform(origin=(0.1, 1.0, 0.5))
    done = WarpProgress(1.0, 1.0, 1.0)
    p = bone_in_vehicle(bone, anchor_end, destination, done, 0.0)
    check("a fully warped bone lands relative to the destination", _close(p, (2.1, 1.2, -0.5)))
    early = WarpProgress(1.0, 0.0, 1.0)
    p = bone_in_vehicle(bone, anchor_end, destination, early, -0.05)
    check("an unwarped height stays on the ground", _close(p, (2.1, 0.95, -0.5)))
    turned = Xform.from_quat((0.0, math.sin(math.pi / 4), 0.0, math.cos(math.pi / 4)), (0.0, 0.0, 0.0))
    p = bone_in_vehicle(bone, Xform(), turned, done, None)
    check("a turned destination turns the bone", _close(p, (0.5, 1.0, -0.1)))

    progress = warp_progress({"SEG_WARP_HORIZONTAL": (0.0, 0.25), "SEG_WARP_VERTICAL": (1.0, 1.5), "SEG_WARP_ORIENTATION": (0.0, 0.25)}, 0.25)
    check("the warp segments run by time", progress == WarpProgress(1.0, 0.0, 1.0))
    check("one SEG_WARP covers all three", warp_progress({"SEG_WARP": (0.0, 0.5)}, 0.25) == WarpProgress(0.5, 0.5, 0.5))
    check("no warp segment is no warp", warp_progress({"SEG_DRIVER_CAMERA": (0.0, 1.0)}, 0.5) is None)

    # A left door, a metre long and tall and five centimetres thick, its skin a single quad.
    box = Box((-1.0, 0.3, -0.9), (-0.95, 1.3, 0.1))
    skin = [((-0.98, 0.3, -0.9), (-0.98, 1.3, -0.9), (-0.98, 1.3, 0.1)), ((-0.98, 0.3, -0.9), (-0.98, 1.3, 0.1), (-0.98, 0.3, 0.1))]
    hit = surface_point((-0.9, 1.0, 0.0), (-1.0, 0.0, 0.0), box, skin)
    check("the palm projects onto the skin", hit.on_mesh and _close(hit.point, (-0.98, 1.0, 0.0)) and _close(hit.normal, (-1.0, 0.0, 0.0)))
    beside = surface_point((-0.9, 1.0, 0.5), (-1.0, 0.0, 0.0), box, skin)
    check("a palm beside the mesh lands on the skin's nearest point", beside.on_mesh and _close(beside.point, (-0.98, 1.0, 0.1)))
    bare = surface_point((-0.9, 1.0, 0.5), (-1.0, 0.0, 0.0), box, [])
    check("without a mesh the palm lands on the box face", not bare.on_mesh and _close(bare.point, (-1.0, 1.0, 0.1)))
    point, thickness, outward, rule = hinge_handle(box, Xform(), SIDE, (-0.97, 0.8, -0.88), None)
    check("a side door's handle is across from its constraint", thickness == 0 and _close(point, (-1.0, 0.8, 0.0)) and rule == "constraint")
    point, _, _, rule = hinge_handle(box, Xform(), SIDE, None, 1.1)
    check("a side door without a constraint hinges at its front, at a given height", _close(point, (-1.0, 1.1, 0.0)) and rule == "kind")
    hood = Box((-0.7, 0.9, -2.2), (0.7, 0.95, -0.9))
    point, _, outward, _ = hinge_handle(hood, Xform(), FRONT, (0.0, 0.95, -0.95), None)
    check("a hood's handle is at its front, on top", _close(point, (0.0, 0.95, -2.07)) and outward == (0.0, 1.0, 0.0))
    check("the slots read by behaviour", (kind_of("FrontLeftDoor"), kind_of("Hood"), kind_of("Trunk"), kind_of("RearDoor")) == (SIDE, FRONT, REAR, REAR))
    return failures


def _close(a, b, tolerance: float = 1e-4) -> bool:
    return all(abs(x - y) <= tolerance for x, y in zip(a, b))
