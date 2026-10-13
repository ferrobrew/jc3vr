"""Read Havok 2014 binary packfiles: skeletons (``.bsk``) and spline-compressed clips (``.ban``).

A packfile (layout 8, 1, 0, 1: 64-bit, little-endian) is a 64-byte header, an optional predicate
block, and section headers of 64 bytes (a 20-byte name, then ``u32`` data start, and the local,
global, virtual, exports, imports, and end offsets, relative to the data start). The
``__classnames__`` section lists the class names (``u32`` signature, a tab byte, the name); the
``__data__`` section holds the objects, and its fixups resolve pointers: local fixups (``u32``
source, destination) within the section, global fixups (``u32`` source, section, destination)
across sections, and virtual fixups (``u32`` source, section, class name offset), which name each
object's class. An ``hkArray`` is an 8-byte pointer and ``i32`` size and capacity.

``hkaSkeleton`` gives the bone parents (``i16``), names, and reference pose (translation, rotation,
and scale, 48 bytes per bone). ``hkaSplineCompressedAnimation`` stores each block of frames as one
B-spline per track component, with quantised control points; ``hkaAnimationBinding`` maps tracks to
bones; ``hkaDefaultAnimatedReferenceFrame`` holds the clip's root motion, one ``(x, y, z, yaw)``
sample per frame.
"""

import math
import struct

from xform import Quat, Vec3, Xform, qnormalize, qyaw

MAGIC = b"W\xe0\xe0W\x10\xc0\xc0\x10"


class Packfile:
    def __init__(self, data: bytes, name: str):
        if data[:8] != MAGIC:
            raise ValueError(f"havok: {name}: not a binary packfile")
        self.data = data
        self.name = name
        section_count = struct.unpack_from("<I", data, 0x14)[0]
        self.version = data[0x28:0x38].split(b"\0")[0].decode()
        _flags, max_predicate, predicate_size = struct.unpack_from("<IHH", data, 0x38)
        p = 0x40 + (predicate_size if max_predicate != 0xFFFF else 0)
        sections = {}
        for _ in range(section_count):
            section_name = data[p : p + 20].split(b"\0")[0].decode()
            sections[section_name] = struct.unpack_from("<7I", data, p + 20)
            p += 64
        start, *_rest, end = sections["__classnames__"]
        classnames = {}
        q = start
        while q < start + end and data[q : q + 4] != b"\xff\xff\xff\xff":
            q += 5
            e = data.index(b"\0", q)
            classnames[q - start] = data[q:e].decode()
            q = e + 1
        start, local, glob, virtual, exports, _imports, _end = sections["__data__"]
        self.base = start
        self.pointers = {}
        for src, dst in _fixups(data, start, local, glob, 2):
            self.pointers[src] = dst
        for src, _section, dst in _fixups(data, start, glob, virtual, 3):
            self.pointers[src] = dst
        self.objects: dict[str, int] = {}
        for src, _section, class_offset in _fixups(data, start, virtual, exports, 3):
            self.objects.setdefault(classnames.get(class_offset, "?"), src)

    def object(self, class_name: str) -> int:
        if class_name not in self.objects:
            raise ValueError(f"havok: {self.name}: no {class_name}")
        return self.objects[class_name]

    def unpack(self, fmt: str, offset: int):
        return struct.unpack_from("<" + fmt, self.data, self.base + offset)

    def array(self, offset: int) -> tuple[int | None, int]:
        return self.pointers.get(offset), self.unpack("i", offset + 8)[0]

    def cstring(self, offset: int) -> str | None:
        target = self.pointers.get(offset)
        if target is None:
            return None
        start = self.base + target
        return self.data[start : self.data.index(b"\0", start)].decode()


class Skeleton:
    def __init__(self, data: bytes, name: str):
        pack = Packfile(data, name)
        k = pack.object("hkaSkeleton")
        parents_at, count = pack.array(k + 0x18)
        self.parents = pack.unpack(f"{count}h", parents_at)
        bones_at, count = pack.array(k + 0x28)
        self.names = [pack.cstring(bones_at + 16 * i) for i in range(count)]
        pose_at, count = pack.array(k + 0x38)
        self.reference = [(pack.unpack("3f", pose_at + 48 * i), pack.unpack("4f", pose_at + 48 * i + 16)) for i in range(count)]
        self.index = {n: i for i, n in enumerate(self.names)}

    def model_space(self, local: list[tuple[Vec3, Quat]]) -> list[Xform]:
        """Each bone's transform in the skeleton's model space, from its local transforms."""
        world: list[Xform] = []
        for i, (t, q) in enumerate(local):
            bone = Xform.from_quat(q, t)
            parent = self.parents[i]
            world.append(bone if parent < 0 else bone.then(world[parent]))
        return world


class Animation:
    def __init__(self, data: bytes, name: str):
        self.name = name
        pack = Packfile(data, name)
        a = pack.object("hkaSplineCompressedAnimation")
        self.duration = pack.unpack("f", a + 0x14)[0]
        self.transform_tracks, self.float_tracks = pack.unpack("2i", a + 0x18)
        self.frames, self.blocks_count, self.frames_per_block, _mask_size = pack.unpack("4i", a + 0x38)
        _block_duration, _inverse, self.frame_duration = pack.unpack("3f", a + 0x48)
        offsets_at, count = pack.array(a + 0x58)
        block_offsets = pack.unpack(f"{count}I", offsets_at)
        data_at, size = pack.array(a + 0x98)
        self._data = pack.data[pack.base + data_at : pack.base + data_at + size]
        binding = pack.object("hkaAnimationBinding")
        tracks_at, count = pack.array(binding + 0x20)
        self.track_to_bone = pack.unpack(f"{count}h", tracks_at) if count else tuple(range(self.transform_tracks))
        self.reference_frame: list[tuple[float, float, float, float]] = []
        if "hkaDefaultAnimatedReferenceFrame" in pack.objects:
            r = pack.object("hkaDefaultAnimatedReferenceFrame")
            up = pack.unpack("3f", r + 0x20)
            if max(abs(up[0]), abs(up[2])) > 1e-4 or up[1] <= 0:
                raise ValueError(f"havok: {name}: the reference frame's up axis is {up}, not +Y")
            samples_at, count = pack.array(r + 0x48)
            self.reference_frame = [pack.unpack("4f", samples_at + 16 * i) for i in range(count)]
        self.quantisations: set[str] = set()
        self._blocks = [self._parse_block(o) for o in block_offsets]

    def local_pose(self, skeleton: Skeleton, t: float) -> list[tuple[Vec3, Quat]]:
        """Each bone's local transform at time ``t``: the clip's track, or the reference pose."""
        local = [(tr, q) for tr, q in skeleton.reference]
        for track, (translation, rotation) in enumerate(self.sample(t)):
            bone = self.track_to_bone[track]
            if 0 <= bone < len(local):
                local[bone] = (translation, rotation)
        return local

    def root(self, t: float) -> Xform:
        """The clip's root motion at time ``t``: where the character's origin is, in the space of
        the character's origin at the clip's start."""
        if not self.reference_frame:
            return Xform()
        frame = t / self.frame_duration
        i = min(max(int(frame), 0), len(self.reference_frame) - 2) if len(self.reference_frame) > 1 else 0
        if len(self.reference_frame) == 1:
            s = self.reference_frame[0]
        else:
            a = min(max(frame - i, 0.0), 1.0)
            s0, s1 = self.reference_frame[i], self.reference_frame[i + 1]
            s = tuple(s0[k] * (1 - a) + s1[k] * a for k in range(4))
        return Xform.from_quat(qyaw(s[3]), (s[0], s[1], s[2]))

    def sample(self, t: float) -> list[tuple[Vec3, Quat]]:
        frame = min(max(t, 0.0), self.duration) / self.frame_duration
        block = min(int(frame / (self.frames_per_block - 1)), self.blocks_count - 1)
        u = frame - block * (self.frames_per_block - 1)
        return [(_eval_vector(pos, u, 0.0), _eval_rotation(rot, u)) for pos, rot, _scale in self._blocks[block]]

    def _parse_block(self, offset: int):
        d = self._data
        masks = [struct.unpack_from("<4B", d, offset + 4 * i) for i in range(self.transform_tracks)]
        o = _align(offset + 4 * self.transform_tracks + self.float_tracks, 4)
        tracks = []
        for quantisation, position_type, rotation_type, scale_type in masks:
            o, position = self._parse_vector(o, position_type, quantisation & 3)
            o = _align(o, 4)
            o, rotation = self._parse_rotation(o, rotation_type, (quantisation >> 2) & 0xF)
            o = _align(o, 4)
            o, scale = self._parse_vector(o, scale_type, (quantisation >> 6) & 3)
            o = _align(o, 4)
            tracks.append((position, rotation, scale))
        return tracks

    def _parse_vector(self, o: int, mask: int, bits: int):
        d = self._data
        spline = [(mask >> (4 + i)) & 1 for i in range(3)]
        static = [(mask >> i) & 1 for i in range(3)]
        if any(spline):
            n, degree = struct.unpack_from("<HB", d, o)
            o += 3
            knots = list(d[o : o + n + degree + 2])
            o = _align(o + n + degree + 2, 4)
            components = []
            for i in range(3):
                if spline[i]:
                    low, high = struct.unpack_from("<2f", d, o)
                    o += 8
                    components.append(("spline", low, high))
                elif static[i]:
                    components.append(("static", struct.unpack_from("<f", d, o)[0]))
                    o += 4
                else:
                    components.append(None)
            points = []
            for _ in range(n + 1):
                point = []
                for c in components:
                    if c and c[0] == "spline":
                        if bits == 0:
                            q = d[o] / 255.0
                            o += 1
                        else:
                            q = struct.unpack_from("<H", d, o)[0] / 65535.0
                            o += 2
                        point.append(c[1] + (c[2] - c[1]) * q)
                    else:
                        point.append(0.0)
                points.append(point)
            return o, ("spline", degree, knots, points, components)
        values = []
        for i in range(3):
            if static[i]:
                values.append(struct.unpack_from("<f", d, o)[0])
                o += 4
            else:
                values.append(None)
        return o, ("static", values)

    def _parse_rotation(self, o: int, mask: int, quantisation: int):
        if quantisation not in ROTATION_FORMATS:
            raise ValueError(f"havok: {self.name}: unknown rotation quantisation {quantisation}")
        label, size, alignment, decode = ROTATION_FORMATS[quantisation]
        d = self._data
        if mask & 0xF0:
            self.quantisations.add(label)
            n, degree = struct.unpack_from("<HB", d, o)
            o += 3
            knots = list(d[o : o + n + degree + 2])
            o = _align(o + n + degree + 2, alignment)
            points = []
            for _ in range(n + 1):
                points.append(decode(d[o : o + size]))
                o += size
            return o, ("spline", degree, knots, points)
        if mask & 0x0F:
            self.quantisations.add(label)
            o = _align(o, alignment)
            return o + size, ("static", decode(d[o : o + size]))
        return o, ("identity",)


def bone_model_transforms(animation: Animation, skeleton: Skeleton, t: float) -> list[Xform]:
    """Every bone's transform at time ``t`` in the space of the character's origin at the clip's
    start: the bone's model-space transform carried by the clip's root motion."""
    root = animation.root(t)
    return [bone.then(root) for bone in skeleton.model_space(animation.local_pose(skeleton, t))]


def _align(o: int, alignment: int) -> int:
    r = o % alignment
    return o + (alignment - r if r else 0)


def _fixups(data: bytes, base: int, start: int, end: int, width: int):
    for p in range(base + start, base + end - 4 * width + 1, 4 * width):
        entry = struct.unpack_from(f"<{width}I", data, p)
        if entry[0] != 0xFFFFFFFF:
            yield entry


def _polar32(b: bytes) -> Quat:
    # 10 bits of the real part's square root (from the top), 18 bits of polar angles packed as
    # phi * phi + theta, and four sign bits.
    v = struct.unpack("<I", b[:4])[0]
    r = ((v >> 18) & 0x3FF) / 1023.0
    r = 1.0 - r * r
    phi_theta = float(v & 0x3FFFF)
    phi = math.floor(math.sqrt(phi_theta))
    theta = 0.0
    if phi > 0:
        theta = (math.pi / 4) * (phi_theta - phi * phi) / phi
        phi = (math.pi / 2) / 511.0 * phi
    magnitude = math.sqrt(max(0.0, 1.0 - r * r))
    q = [math.sin(phi) * math.cos(theta) * magnitude, math.sin(phi) * math.sin(theta) * magnitude, math.cos(phi) * magnitude, r]
    for i, bit in enumerate((0x10000000, 0x20000000, 0x40000000, 0x80000000)):
        if v & bit:
            q[i] = -q[i]
    return tuple(q)


def _three_comp40(b: bytes) -> Quat:
    # Three 12-bit components, the index of the dropped (largest) component, and its sign.
    v = int.from_bytes(b[:5], "little")
    mask = (1 << 12) - 1
    fraction = 1.0 / (mask >> 1) / math.sqrt(2)
    c = [(((v >> (12 * i)) & mask) - (mask >> 1)) * fraction for i in range(3)]
    shift = (v >> 36) & 3
    w = math.sqrt(max(0.0, 1.0 - sum(x * x for x in c)))
    if (v >> 38) & 1:
        w = -w
    c.insert(shift, w)
    return tuple(c)


def _three_comp48(b: bytes) -> Quat:
    # Three 15-bit components; the dropped component's index is in the top bits of the first two,
    # and its sign in the top bit of the third.
    x, y, z = struct.unpack("<3H", b[:6])
    mask = (1 << 15) - 1
    fraction = 1.0 / (mask >> 1) / math.sqrt(2)
    shift = ((y >> 14) & 2) | ((x >> 15) & 1)
    c = [((t & mask) - (mask >> 1)) * fraction for t in (x, y, z)]
    w = math.sqrt(max(0.0, 1.0 - sum(t * t for t in c)))
    if z >> 15:
        w = -w
    c.insert(shift, w)
    return tuple(c)


def _uncompressed(b: bytes) -> Quat:
    return struct.unpack("<4f", b[:16])


def _unsupported(label: str):
    def decode(_b: bytes) -> Quat:
        raise ValueError(f"havok: the {label} rotation quantisation is not supported")

    return decode


# Havok's rotation quantisations: label, size, alignment, and decoder.
ROTATION_FORMATS = {
    0: ("POLAR32", 4, 4, _polar32),
    1: ("THREECOMP40", 5, 1, _three_comp40),
    2: ("THREECOMP48", 6, 2, _three_comp48),
    3: ("THREECOMP24", 3, 1, _unsupported("THREECOMP24")),
    4: ("STRAIGHT16", 2, 2, _unsupported("STRAIGHT16")),
    5: ("UNCOMPRESSED", 16, 4, _uncompressed),
}


def _span(degree: int, u: float, n: int, knots: list[int]) -> int:
    if u >= knots[n]:
        return n - 1
    low, high = degree, n
    mid = (low + high) // 2
    while u < knots[mid] or u >= knots[mid + 1]:
        if u < knots[mid]:
            high = mid
        else:
            low = mid
        mid = (low + high) // 2
    return mid


def _de_boor(degree: int, u: float, knots: list[int], points: list) -> list[float]:
    k = _span(degree, u, len(points), knots)
    d = [list(points[j + k - degree]) for j in range(degree + 1)]
    for r in range(1, degree + 1):
        for j in range(degree, r - 1, -1):
            den = knots[j + 1 + k - r] - knots[j + k - degree]
            a = 0.0 if den == 0 else (u - knots[j + k - degree]) / den
            d[j] = [(1 - a) * x + a * y for x, y in zip(d[j - 1], d[j])]
    return d[degree]


def _eval_vector(v, u: float, default: float) -> Vec3:
    if v[0] == "static":
        return tuple(default if c is None else c for c in v[1])
    _, degree, knots, points, components = v
    p = _de_boor(degree, u, knots, points)
    return tuple((p[i] if c[0] == "spline" else c[1]) if c else default for i, c in enumerate(components))


def _eval_rotation(r, u: float) -> Quat:
    if r[0] == "identity":
        return (0.0, 0.0, 0.0, 1.0)
    if r[0] == "static":
        return qnormalize(r[1])
    _, degree, knots, points = r
    # Keep the control points in one hemisphere so that the blend takes the short way.
    aligned = [points[0]]
    for q in points[1:]:
        if sum(a * b for a, b in zip(q, aligned[-1])) < 0:
            q = tuple(-c for c in q)
        aligned.append(q)
    return qnormalize(tuple(_de_boor(degree, u, knots, aligned)))

