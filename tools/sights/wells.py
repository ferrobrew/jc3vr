"""Derive a weapon's reload geometry from its model: where a round or magazine goes in, and where the
hand grabs to chamber.

Everything is in weapon space: metres, +Y up, -Z toward the muzzle, and +X to the gun's right. That
is the frame of the gun's runtime world matrix and of each model part's ``world`` matrix.

The magazine well's mouth is found by casting lines through the gun's triangles (``Projection``).
With a separate magazine part, lines parallel to the part's +Y axis run up through the footprint of
the magazine's feed end, and the mouth is where they first meet the body. Without a part, the
magazine is modelled into the body, and the mouth is the base of the lowest column of the body: the
grip base on a pistol, the magazine base on an SMG. Rockets enter the front of the tube, revolvers
load at the cylinder's rear face, the pump shotgun at the port in front of the trigger guard, and
the break-action shotgun at its breech.
"""

import math
from dataclasses import dataclass, field

from weapon import Part, Vec3, Weapon

# The kinds of reload, and the actions that chamber a round.
MAGAZINE, SHELL, ROCKET, CYLINDER, NONE = "magazine", "shell", "rocket", "cylinder", "none"
SLIDE, CHARGING_HANDLE, BOLT, PUMP = "slide", "charging_handle", "bolt", "pump"

# The grades of a well: the mouth and axis came from a magazine part, from the body mesh, from a
# class default, or could not be derived at all.
PART, MESH, DEFAULT, FAILED = "part", "mesh", "default", "failed"

# Component names that carry the round or magazine, which is not part of the gun's body.
ROUND_PARTS = ("model.magazine", "model.cartridge_full", "model.rocket_pod", "model.rpg")
NOT_BODY = ("magazine", "cartridge", "rocket_pod", "model.rpg", "ammo")

# Magazine parts.
FEED_SECTION = 0.015  # the feed end is the part's vertices within this distance of its top
FOOTPRINT_MARGIN = 0.006  # lines this far outside the feed end's footprint catch a well's rim
RAY_SPACING = 0.003
MOUTH_PERCENTILE = 0.25  # the mouth is the first contact of this fraction of the lines
MOUTH_BELOW_TOP = 0.15  # a contact this far below the feed end, or further, is another feature
MOUTH_ABOVE_TOP = 0.05

# Magazines and grips modelled into the body.
BASE_BAND = 0.006  # the base is the body's vertices within this distance of its lowest point
BASE_REACH = 0.04  # and within this distance of the lowest vertex along the barrel
# The heights above the column's lowest point where its centre line is sampled, clear of a slanted
# base.
COLUMN_HEIGHTS = (0.028, 0.036, 0.044, 0.052, 0.06)
COLUMN_STEP = 0.005
COLUMN_WIDENING = 1.5  # the column ends where it widens by this factor
COLUMN_MAX_LENGTH = 0.25
COLUMN_MAX_TILT_DEG = 40.0
GRIP_REACH = 0.05  # a magazine base within this distance of the origin along the barrel is the grip

# Pistol slides.
SLIDE_REAR = 0.01  # the slide's rear end is its vertices within this distance of its back

# Rockets.
TUBE_RADIUS = 0.07  # lines within this radius of the round's axis find the tube's front face
TUBE_PERCENTILE = 0.1

# Width profiles (revolver cylinders and break-action barrels).
PROFILE_BIN = 0.003
CYLINDER_BAND = (-0.045, 0.02)  # the height band around the bore searched for the cylinder
CYLINDER_WIDE = 0.85  # a cylinder bin is at least this fraction of the widest bin
CYLINDER_SIDE = 0.6  # the lines finding the cylinder's rear face run at this fraction of its radius
BREECH_BAND = 0.03
BREECH_SKIP = 0.01  # the profile starts this far behind the muzzle, past any muzzle detail
BREECH_STEP = 0.005  # the frame is at least this much wider than the barrel
BREECH_RUN = 3  # bins

# Pump shotguns.
PROFILE_STEP = 0.005
GUARD_BAR = 0.012  # a trigger guard's bar is thinner than this
GUARD_HOLE = 0.006  # and has a hole at least this tall above it
GUARD_REACH = 0.15  # the guard is within this distance ahead of the origin, which sits at the grip
PORT_OPENING = 0.008  # the port is an opening this much above the receiver's underside
PUMP_TOLERANCE = 0.003
PUMP_MIN_LENGTH = 0.08
PUMP_STEP = 0.004  # a pump's underside sits at least this far below the receiver's at the port

# Side handles: charging handles and bolt handles stick out from the right of the receiver.
HANDLE_BAND = (-0.06, 0.035)  # the height band around the bore
HANDLE_FRONT, HANDLE_REAR = 0.15, 0.2  # the fractions of the gun's length excluded at each end
HANDLE_PROMINENCE = 0.006  # how far the handle reaches past the receiver around it
HANDLE_CORE = 0.015  # the handle is narrower than twice this along the barrel
HANDLE_WINDOW = 0.05  # the receiver around the handle is within this distance of it
HANDLE_TIP = 0.004

# The class defaults used when a value cannot be measured.
DEFAULT_TRAVEL = {SLIDE: 0.025, CHARGING_HANDLE: 0.07, BOLT: 0.08, PUMP: 0.08}
DEFAULT_SMG_TRAVEL = 0.05
DEFAULT_HANDLE_ABOVE_BORE = 0.05  # the default handle is the receiver's top over the grip, below this
# The default mouths, as (height, position along the barrel). The cylinder's height is relative to
# the bore's.
DEFAULT_MAGAZINE_MOUTH = (-0.03, 0.0)
DEFAULT_SHELL_MOUTH = (0.075, -0.15)
DEFAULT_CYLINDER_MOUTH = (-0.012, -0.055)


@dataclass
class Well:
    kind: str
    grade: str
    mouth: Vec3 | None = None
    axis: Vec3 | None = None
    part: Part | None = None
    part_top: float | None = None
    part_length: float | None = None
    action: str = NONE
    handle: Vec3 | None = None
    travel: float | None = None
    # The fields that hold class defaults rather than measurements, and other remarks.
    notes: list[str] = field(default_factory=list)
    # Why a measured value is doubtful, if it is.
    doubts: list[str] = field(default_factory=list)


def derive(weapon: Weapon, family: str) -> Well:
    """The weapon's well. ``family`` is the class word of the entity name (``pistol``, ``smg``,
    ``assault``, ...), which, with the weapon's class, decides the kind of reload."""
    body = [p for p in weapon.parts if not any(s in p.name for s in NOT_BODY)]
    round_part = next((p for n in ROUND_PARTS for p in weapon.parts if p.name == n), None)
    if not body or not weapon.fire_points:
        return Well(NONE, DEFAULT, notes=["nothing to load"])
    gun = _Gun(weapon, body)

    if family == "pistol":
        well = _mesh_magazine(gun, MAGAZINE)
        _slide(well, weapon)
    elif family == "handgun":
        well = _cylinder(gun)
    elif family == "smg":
        well = _mesh_magazine(gun, MAGAZINE)
        _side_handle(well, gun, CHARGING_HANDLE, DEFAULT_SMG_TRAVEL)
    elif family == "shotgun":
        well = _shotgun(gun)
    elif family in ("assault", "machine", "sniper", "grenade") and round_part is not None:
        well = _part_magazine(gun, round_part, _part_axis(round_part))
        if family == "sniper":
            _bolt(well, weapon, gun)
        elif family == "grenade":
            # The launchers' side profiles are dominated by their drums and barrel clamps, so a
            # protrusion there is not a handle.
            well.notes.append("no chambering action")
        else:
            _side_handle(well, gun, CHARGING_HANDLE, DEFAULT_TRAVEL[CHARGING_HANDLE])
    elif round_part is not None and round_part.name == "model.rocket_pod":
        # The pod slides in from the rear, so its feed end is its front.
        well = _part_magazine(gun, round_part, (0.0, 0.0, -1.0))
        well.kind = ROCKET
        well.notes.append("the pod enters from the rear; the mouth is the rear of its housing")
    elif round_part is not None and (weapon.class_name.startswith("CRPG") or weapon.class_name == "CMortarWeapon"):
        well = _rocket(gun, round_part)
    else:
        return Well(NONE, DEFAULT, notes=["nothing to load"])
    if well.mouth is None:
        _default_mouth(well, gun)
    return well


class Projection:
    """The gun's triangles seen along one axis, for casting lines parallel to it.

    ``u`` and ``v`` complete the axis to a right-handed basis. A line is given by its (u, v)
    coordinates, and its hits are the coordinates along the axis where it crosses a triangle.
    """

    CELL = 0.01

    def __init__(self, triangles: list[tuple[Vec3, Vec3, Vec3]], axis: Vec3):
        self.axis = _unit(axis)
        self.u, self.v = _basis(self.axis)
        self._grid: dict[tuple[int, int], list] = {}
        for tri in triangles:
            projected = [(_dot(p, self.u), _dot(p, self.v), _dot(p, self.axis)) for p in tri]
            us = [p[0] for p in projected]
            vs = [p[1] for p in projected]
            for i in range(math.floor(min(us) / self.CELL), math.floor(max(us) / self.CELL) + 1):
                for j in range(math.floor(min(vs) / self.CELL), math.floor(max(vs) / self.CELL) + 1):
                    self._grid.setdefault((i, j), []).append(projected)

    def coordinates(self, p: Vec3) -> tuple[float, float, float]:
        return _dot(p, self.u), _dot(p, self.v), _dot(p, self.axis)

    def point(self, u: float, v: float, s: float) -> Vec3:
        return _add(_add(_scale(self.u, u), _scale(self.v, v)), _scale(self.axis, s))

    def hits(self, u: float, v: float) -> list[float]:
        out = []
        for (au, av, as_), (bu, bv, bs), (cu, cv, cs) in self._grid.get((math.floor(u / self.CELL), math.floor(v / self.CELL)), ()):
            det = (bv - cv) * (au - cu) + (cu - bu) * (av - cv)
            if abs(det) < 1e-12:
                continue
            l1 = ((bv - cv) * (u - cu) + (cu - bu) * (v - cv)) / det
            l2 = ((cv - av) * (u - cu) + (au - cu) * (v - cv)) / det
            l3 = 1.0 - l1 - l2
            if l1 >= 0 and l2 >= 0 and l3 >= 0:
                out.append(l1 * as_ + l2 * bs + l3 * cs)
        return sorted(out)


class _Gun:
    """The gun's body (every part but the round) and the projections cast through it."""

    def __init__(self, weapon: Weapon, body: list[Part]):
        self.fire = weapon.fire_points[0].translation
        self.vertices = [p for part in body for p in part.positions]
        self.triangles = [tuple(part.positions[i] for i in tri) for part in body for tri in part.triangles]
        self.z_rear = max(p[2] for p in self.vertices)
        self.length = self.z_rear - self.fire[2]
        self._projections: dict[Vec3, Projection] = {}

    def along(self, axis: Vec3) -> Projection:
        key = tuple(round(c, 6) for c in axis)
        if key not in self._projections:
            self._projections[key] = Projection(self.triangles, axis)
        return self._projections[key]


def _default_mouth(well: Well, gun: _Gun) -> None:
    """A class default for a mouth that could not be measured, placed from the measured weapons of
    the class: a magazine or shells under the receiver ahead of the grip, a round at the muzzle,
    and a revolver's cylinder face below the bore ahead of the grip."""
    fx, fy, fz = gun.fire
    defaults = {
        MAGAZINE: ((0.0, DEFAULT_MAGAZINE_MOUTH[0], DEFAULT_MAGAZINE_MOUTH[1]), (0.0, 1.0, 0.0)),
        SHELL: ((0.0, DEFAULT_SHELL_MOUTH[0], DEFAULT_SHELL_MOUTH[1]), (0.0, 1.0, 0.0)),
        ROCKET: ((fx, fy, fz), (0.0, 0.0, 1.0)),
        CYLINDER: ((0.0, fy + DEFAULT_CYLINDER_MOUTH[0], DEFAULT_CYLINDER_MOUTH[1]), (0.0, 0.0, -1.0)),
    }
    if well.kind not in defaults:
        well.grade = FAILED
        return
    well.mouth, well.axis = defaults[well.kind]
    well.grade = DEFAULT
    well.notes.append("mouth and axis: class default")


def _part_axis(part: Part) -> Vec3:
    # The part's local +Y, the second row of its row-major matrix: an upright magazine travels up.
    m = part.matrix
    return _unit((m[4], m[5], m[6])) if m is not None else (0.0, 1.0, 0.0)


def _part_magazine(gun: _Gun, part: Part, axis: Vec3) -> Well:
    """A magazine part: its feed end, and the first contact with the body above it."""
    projection = gun.along(axis)
    origin = part.matrix[12:15] if part.matrix is not None else (0.0, 0.0, 0.0)
    origin_s = _dot(origin, projection.axis)
    coords = [projection.coordinates(p) for p in part.positions]
    top = max(c[2] for c in coords)
    bottom = min(c[2] for c in coords)
    well = Well(MAGAZINE, PART, axis=projection.axis, part=part, part_top=top - origin_s, part_length=top - bottom)

    feed = [c for c in coords if c[2] >= top - FEED_SECTION]
    contacts, inside = _contacts(projection, feed, top)
    if not inside:
        # The body is beside the feed end rather than over it (a box hung off the side): the mouth
        # is the body's underside over the rest of the part.
        contacts, inside = _contacts(projection, coords, top)
        if inside:
            well.doubts.append("the body is beside the magazine's feed end; the mouth is its underside over the magazine")
    if not contacts:
        u = (min(c[0] for c in feed) + max(c[0] for c in feed)) / 2
        v = (min(c[1] for c in feed) + max(c[1] for c in feed)) / 2
        well.mouth = projection.point(u, v, top)
        well.doubts.append("no body above the magazine; the mouth is the magazine's top")
        return well
    if not inside:
        inside = [(u, v) for u, v, _ in contacts]
        well.doubts.append("the body is only around the magazine's footprint")
    level = _percentile([s for _, _, s in contacts], MOUTH_PERCENTILE)
    cu = sum(u for u, _ in inside) / len(inside)
    cv = sum(v for _, v in inside) / len(inside)
    well.mouth = projection.point(cu, cv, level)
    return well


def _contacts(projection: Projection, footprint: list[tuple[float, float, float]], top: float):
    """The first contact with the body of each line over the footprint (widened by the margin)
    within the window around the feed end, as (u, v, s), and the lines inside the footprint
    proper that made one."""
    u0, u1 = min(c[0] for c in footprint), max(c[0] for c in footprint)
    v0, v1 = min(c[1] for c in footprint), max(c[1] for c in footprint)
    contacts = []
    inside = []
    for u in _steps(u0 - FOOTPRINT_MARGIN, u1 + FOOTPRINT_MARGIN, RAY_SPACING):
        for v in _steps(v0 - FOOTPRINT_MARGIN, v1 + FOOTPRINT_MARGIN, RAY_SPACING):
            first = next((s for s in projection.hits(u, v) if top - MOUTH_BELOW_TOP <= s <= top + MOUTH_ABOVE_TOP), None)
            if first is None:
                continue
            contacts.append((u, v, first))
            if u0 <= u <= u1 and v0 <= v <= v1:
                inside.append((u, v))
    return contacts, inside


def _mesh_magazine(gun: _Gun, kind: str) -> Well:
    """A magazine or grip modelled into the body: the base of the body's lowest column."""
    well = Well(kind, MESH)
    lowest = min(gun.vertices, key=lambda p: p[1])
    base = [p for p in gun.vertices if p[1] <= lowest[1] + BASE_BAND and abs(p[2] - lowest[2]) <= BASE_REACH]
    x = (min(p[0] for p in base) + max(p[0] for p in base)) / 2
    z = (min(p[2] for p in base) + max(p[2] for p in base)) / 2

    # The column's centre line, sampled by lines along the barrel at rising heights, each starting
    # from the previous centre so that a slanted column is followed.
    centres = []
    for h in COLUMN_HEIGHTS:
        span = _span(gun, x, lowest[1] + h, z)
        if span is None:
            break
        z = (span[0] + span[1]) / 2
        centres.append((lowest[1] + h, z, span[1] - span[0]))
    if len(centres) < 3:
        well.axis = (0.0, 1.0, 0.0)
        well.mouth = (x, lowest[1], z)
        well.doubts.append("the column above the lowest point could not be traced; the axis is +Y")
        return well
    slope = _fit_slope([(h, c) for h, c, _ in centres])
    axis = _unit((0.0, 1.0, slope))
    if math.degrees(math.acos(axis[1])) > COLUMN_MAX_TILT_DEG:
        well.doubts.append(f"the column tilts {math.degrees(math.acos(axis[1])):.0f} deg; using +Y")
        axis = (0.0, 1.0, 0.0)
    well.axis = axis

    # The mouth is where the centre line leaves the column's base.
    h0, z0, _ = centres[0]
    projection = gun.along(axis)
    start = (x, h0, z0)
    u, v, s0 = projection.coordinates(start)
    # The outermost crossing, not the first: a separate baseplate leaves faces inside the column.
    exits = [s for s in projection.hits(u, v) if s <= s0 and projection.point(u, v, s)[1] >= lowest[1] - 1e-4]
    if not exits:
        well.mouth = (x, lowest[1], z0)
        well.doubts.append("the centre line does not leave the column's base")
    else:
        well.mouth = projection.point(u, v, min(exits))

    if abs(well.mouth[2]) > GRIP_REACH:
        well.part_length = _column_length(gun, x, lowest[1], centres)
        well.notes.append("magazine modelled into the body; the mouth is its base")
    else:
        well.notes.append("magazine inside the grip; the mouth is the grip base")
    return well


def _span(gun: _Gun, x: float, y: float, z: float) -> tuple[float, float] | None:
    """The interval along the barrel of the body around ``z`` at the given height and offset."""
    projection = gun.along((0.0, 0.0, 1.0))
    u, v, _ = projection.coordinates((x, y, 0.0))
    hits = projection.hits(u, v)
    front = [s for s in hits if s < z]
    rear = [s for s in hits if s > z]
    if not front or not rear:
        return None
    return max(front), min(rear)


def _column_length(gun: _Gun, x: float, bottom: float, centres: list) -> float | None:
    """How far the column rises from its base before it widens into the receiver."""
    width = sorted(w for _, _, w in centres[:3])[1]
    z = centres[0][1]
    h = centres[0][0]
    while h - bottom < COLUMN_MAX_LENGTH:
        h += COLUMN_STEP
        span = _span(gun, x, h, z)
        if span is None or span[1] - span[0] > COLUMN_WIDENING * width:
            return h - bottom
        z = (span[0] + span[1]) / 2
    return None


def _slide(well: Well, weapon: Weapon) -> None:
    """A pistol slide, a separate recoiling part: the grab point is the middle of its rear end."""
    slide = next((p for p in weapon.parts if p.class_name == "CBarrelRecoilWeaponComponent"), None)
    well.action = SLIDE
    well.travel = DEFAULT_TRAVEL[SLIDE]
    well.notes.append("travel: class default")
    if slide is None:
        well.notes.append("handle: no slide part")
        return
    rear = max(p[2] for p in slide.positions)
    end = [p for p in slide.positions if p[2] >= rear - SLIDE_REAR]
    ys = [p[1] for p in end]
    xs = [p[0] for p in end]
    well.handle = ((min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2, rear)
    well.notes.append(f"handle: the rear of {slide.name}")


def _cylinder(gun: _Gun) -> Well:
    """A revolver: the rear face of the cylinder, the widest part of the gun around the bore."""
    well = Well(CYLINDER, MESH, axis=(0.0, 0.0, -1.0))
    fy = gun.fire[1]
    profile = _width_profile(gun, fy + CYLINDER_BAND[0], fy + CYLINDER_BAND[1], gun.fire[2], gun.z_rear)
    widest = max(w for _, w in profile)
    run = _longest_run([z for z, w in profile if w >= CYLINDER_WIDE * widest], PROFILE_BIN * 2.5)
    front, rear = run[0], run[-1] + PROFILE_BIN
    band = [p for p in gun.vertices if fy + CYLINDER_BAND[0] <= p[1] <= fy + CYLINDER_BAND[1]]
    cylinder = [p for p in band if front - PROFILE_BIN <= p[2] <= rear + PROFILE_BIN and abs(p[0]) >= 0.5 * widest]
    centre = (min(p[1] for p in cylinder) + max(p[1] for p in cylinder)) / 2
    # The frame behind the cylinder can be as wide as it, so the rear face is taken from lines
    # along the barrel through the cylinder's side: their second crossing is the rear face.
    projection = gun.along((0.0, 0.0, 1.0))
    faces = []
    for x in (-CYLINDER_SIDE * widest, CYLINDER_SIDE * widest):
        for y in (centre - 0.005, centre + 0.005):
            u, v, _ = projection.coordinates((x, y, 0.0))
            hits = [z for z in projection.hits(u, v) if front - 2 * PROFILE_BIN <= z <= rear + 2 * PROFILE_BIN]
            if len(hits) >= 2:
                faces.append(hits[1])
    if faces:
        rear = sorted(faces)[len(faces) // 2]
    else:
        well.doubts.append("the cylinder's rear face was not crossed; using the end of its widest run")
    well.mouth = (0.0, centre, rear)
    well.notes.append("the mouth is the centre of the cylinder's rear face")
    if rear - front < 0.03:
        well.doubts.append(f"the cylinder is only {rear - front:.3f} m long")
    return well


def _shotgun(gun: _Gun) -> Well:
    """A box magazine when the gun's lowest point is apart from the grip, else shells: through a
    port with a pump, or into the breech without one."""
    lowest = min(gun.vertices, key=lambda p: p[1])
    if abs(lowest[2]) > GRIP_REACH:
        # A box magazine apart from the grip, modelled into the body.
        well = _mesh_magazine(gun, MAGAZINE)
        _side_handle(well, gun, CHARGING_HANDLE, DEFAULT_TRAVEL[CHARGING_HANDLE], required=False)
        return well
    pump = _pump(gun)
    if pump is None:
        return _breech(gun)
    return _port(gun, pump)


def _breech(gun: _Gun) -> Well:
    """A break action: the rear end of the barrels, where the frame widens behind them."""
    well = Well(SHELL, MESH, axis=(0.0, 0.0, -1.0))
    fx, fy, fz = gun.fire
    # The fire point need not sit at the muzzle, so the profile starts at the body's front.
    front = min(p[2] for p in gun.vertices if abs(p[1] - fy) <= BREECH_BAND)
    profile = _width_profile(gun, fy - BREECH_BAND, fy + BREECH_BAND, front + BREECH_SKIP, gun.z_rear)
    barrel = sorted(w for _, w in profile[:5])[2]
    run = 0
    for i, (z, w) in enumerate(profile):
        run = run + 1 if w > barrel + BREECH_STEP else 0
        if run == BREECH_RUN:
            well.mouth = (fx, fy, profile[i - BREECH_RUN + 1][0])
            well.notes.append("the mouth is the breech, at the bore's height")
            return well
    well.grade = FAILED
    well.doubts.append("the frame never widens behind the barrel")
    return well


def _bottom_profile(gun: _Gun, z0: float, z1: float) -> list[tuple[float, list[float]]]:
    projection = gun.along((0.0, 1.0, 0.0))
    out = []
    for z in _steps(z0, z1, PROFILE_STEP):
        u, v, _ = projection.coordinates((0.0, 0.0, z))
        hits = projection.hits(u, v)
        if hits:
            out.append((z, hits))
    return out


def _pump(gun: _Gun):
    """The trigger guard's front and the pump's extent, from the body's underside along the
    centreline, or ``None`` when the gun has no pump."""
    profile = _bottom_profile(gun, gun.fire[2], 0.0)
    guard = [z for z, hits in profile if len(hits) >= 3 and hits[1] - hits[0] < GUARD_BAR and hits[2] - hits[1] >= GUARD_HOLE and z > -GUARD_REACH]
    if not guard:
        return None
    guard_front = min(guard)
    ahead = [(z, hits[0]) for z, hits in profile if z < guard_front - 0.03]
    if not ahead:
        return None
    floor = min(y for _, y in ahead)
    low = [z for z, y in ahead if y <= floor + PUMP_TOLERANCE]
    run = _longest_run(low, PROFILE_STEP * 1.5)
    if run[-1] - run[0] < PUMP_MIN_LENGTH:
        return None
    bottom = {z: hits[0] for z, hits in profile}
    port = min(bottom, key=lambda k: abs(k - (run[-1] + guard_front) / 2))
    if bottom[port] < floor + PUMP_STEP:
        return None
    return guard_front, run[0], run[-1], bottom


def _port(gun: _Gun, pump) -> Well:
    """A pump shotgun: the loading port under the receiver, between the pump and the guard. The
    port is an opening in the receiver's underside; without one, the mouth is midway."""
    guard_front, pump_front, pump_rear, bottom = pump
    well = Well(SHELL, MESH, axis=(0.0, 1.0, 0.0), action=PUMP, travel=DEFAULT_TRAVEL[PUMP])
    quarter = (guard_front - pump_rear) / 4
    span = {z: y for z, y in bottom.items() if pump_rear + quarter <= z <= guard_front - quarter}
    underside = min(span.values())
    opening = [z for z, y in span.items() if y > underside + PORT_OPENING]
    if opening:
        z = (min(opening) + max(opening)) / 2
        well.notes.append("the mouth is the loading port's opening, between the pump and the trigger guard")
    else:
        z = (pump_rear + guard_front) / 2
        well.notes.append("the mouth is midway between the pump and the trigger guard")
        well.doubts.append("no opening in the receiver's underside")
    well.mouth = (0.0, underside, z)
    grip = [p for p in gun.vertices if pump_front <= p[2] <= pump_rear and p[1] < gun.fire[1] - 0.005]
    well.handle = (0.0, (min(p[1] for p in grip) + max(p[1] for p in grip)) / 2, (pump_front + pump_rear) / 2)
    well.notes.append("handle: the pump's centre")
    well.notes.append("travel: class default")
    return well


def _rocket(gun: _Gun, part: Part) -> Well:
    """A round loaded into the muzzle: the front face of the tube around the round's axis."""
    axis = (0.0, 0.0, 1.0)
    xs = [p[0] for p in part.positions]
    ys = [p[1] for p in part.positions]
    zs = [p[2] for p in part.positions]
    cx, cy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
    origin_z = part.matrix[14] if part.matrix is not None else 0.0
    well = Well(ROCKET, PART, axis=axis, part=part, part_top=max(zs) - origin_z, part_length=max(zs) - min(zs))
    projection = gun.along(axis)
    contacts = []
    for dx in _steps(-TUBE_RADIUS, TUBE_RADIUS, RAY_SPACING):
        for dy in _steps(-TUBE_RADIUS, TUBE_RADIUS, RAY_SPACING):
            if dx * dx + dy * dy > TUBE_RADIUS**2:
                continue
            u, v, _ = projection.coordinates((cx + dx, cy + dy, 0.0))
            hits = projection.hits(u, v)
            if hits:
                contacts.append(hits[0])
    if not contacts:
        well.doubts.append("no tube around the round")
        return well
    well.mouth = (cx, cy, _percentile(contacts, TUBE_PERCENTILE))
    well.notes.append("the mouth is the front of the tube")
    return well


def _bolt(well: Well, weapon: Weapon, gun: _Gun) -> None:
    """A bolt or charging handle: the recoiling ejection port part's outer tip, or the right-side
    protrusion of the receiver."""
    port = next((p for p in weapon.parts if p.class_name == "CBarrelRecoilWeaponComponent"), None)
    if port is None:
        _side_handle(well, gun, BOLT, DEFAULT_TRAVEL[BOLT])
        return
    well.action = BOLT
    well.travel = DEFAULT_TRAVEL[BOLT]
    well.handle = _tip(port.positions)
    well.notes.append(f"handle: the tip of {port.name}")
    well.notes.append("travel: class default")


def _side_handle(well: Well, gun: _Gun, action: str, travel: float, required: bool = True) -> None:
    """The handle as a narrow protrusion from the right of the receiver near the bore, or a class
    default. With ``required`` false, a gun without a protrusion has no action."""
    fy, fz = gun.fire[1], gun.fire[2]
    z0 = fz + HANDLE_FRONT * gun.length
    z1 = gun.z_rear - HANDLE_REAR * gun.length
    y0, y1 = fy + HANDLE_BAND[0], fy + HANDLE_BAND[1]
    profile = _width_profile(gun, y0, y1, z0, z1, side=1.0)
    peak = None
    for z, w in profile:
        # A protrusion stands out from the profile around it, past its own width.
        around = sorted(o for oz, o in profile if HANDLE_CORE < abs(oz - z) <= HANDLE_WINDOW)
        if not around:
            continue
        prominence = w - around[len(around) // 2]
        if prominence >= HANDLE_PROMINENCE and (peak is None or prominence > peak[2]):
            peak = (z, w, prominence)
    handle = None
    if peak is not None:
        z, w, _ = peak
        tip = [p for p in gun.vertices if y0 <= p[1] <= y1 and abs(p[2] - z) <= HANDLE_CORE and p[0] >= w - HANDLE_TIP]
        if tip:
            handle = _tip(tip)
    if handle is None and not required:
        return
    well.action = action
    well.travel = travel
    well.notes.append("travel: class default")
    if handle is not None:
        well.handle = handle
        well.notes.append("handle: the receiver's right-side protrusion")
        return
    # The default: the top of the receiver over the grip, below any optic or carry handle.
    projection = gun.along((0.0, 1.0, 0.0))
    u, v, _ = projection.coordinates((0.0, 0.0, 0.0))
    tops = [y for y in projection.hits(u, v) if y <= fy + DEFAULT_HANDLE_ABOVE_BORE]
    well.handle = (0.0, max(tops) if tops else fy + DEFAULT_HANDLE_ABOVE_BORE, 0.0)
    well.notes.append("handle: class default, the top of the receiver over the grip")


def _tip(points: list[Vec3]) -> Vec3:
    reach = max(p[0] for p in points)
    tip = [p for p in points if p[0] >= reach - HANDLE_TIP]
    return tuple(sum(p[i] for p in tip) / len(tip) for i in range(3))


def _width_profile(gun: _Gun, y0: float, y1: float, z0: float, z1: float, side: float | None = None) -> list[tuple[float, float]]:
    """The body's largest |x| (or, with ``side``, its largest x on that side) per bin along the
    barrel, over the height band from ``y0`` to ``y1``, from lines cast across the gun."""
    projection = gun.along((1.0, 0.0, 0.0))
    out = []
    for z in _steps(z0, z1, PROFILE_BIN):
        widest = None
        for y in _steps(y0, y1, PROFILE_BIN):
            u, v, _ = projection.coordinates((0.0, y, z))
            for x in projection.hits(u, v):
                w = abs(x) if side is None else x * side
                widest = w if widest is None else max(widest, w)
        if widest is not None:
            out.append((z, widest))
    return out


def _longest_run(values: list[float], gap: float) -> list[float]:
    best: list[float] = []
    run: list[float] = []
    for value in sorted(values):
        if run and value - run[-1] > gap:
            run = []
        run.append(value)
        if run[-1] - run[0] > (best[-1] - best[0] if best else -1.0):
            best = list(run)
    return best


def _fit_slope(points: list[tuple[float, float]]) -> float:
    n = len(points)
    mh = sum(h for h, _ in points) / n
    mz = sum(z for _, z in points) / n
    num = sum((h - mh) * (z - mz) for h, z in points)
    den = sum((h - mh) ** 2 for h, _ in points)
    return num / den if den else 0.0


def _percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(fraction * len(ordered)))]


def _steps(start: float, end: float, step: float):
    n = int(math.floor((end - start) / step))
    return [start + i * step for i in range(n + 1)]


def _basis(axis: Vec3) -> tuple[Vec3, Vec3]:
    reference = (1.0, 0.0, 0.0) if abs(axis[0]) < 0.9 else (0.0, 1.0, 0.0)
    u = _unit(_add(reference, _scale(axis, -_dot(reference, axis))))
    v = _cross(axis, u)
    return u, v


def _dot(a: Vec3, b: Vec3) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _add(a: Vec3, b: Vec3) -> Vec3:
    return a[0] + b[0], a[1] + b[1], a[2] + b[2]


def _scale(a: Vec3, k: float) -> Vec3:
    return a[0] * k, a[1] * k, a[2] * k


def _cross(a: Vec3, b: Vec3) -> Vec3:
    return a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]


def _unit(a: Vec3) -> Vec3:
    n = math.sqrt(_dot(a, a))
    return _scale(a, 1.0 / n)
