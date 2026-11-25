"""Rigid transforms and quaternions, in the engine's row-vector convention.

A transform is a rotation, held as its three axes, and an origin. It maps a point ``p`` to
``p.x * x + p.y * y + p.z * z + origin``, which is how the engine's row-major 4x4 matrices act:
the rows are the axes and the translation. ``a.then(b)`` applies ``a`` first and ``b`` second,
the engine's ``a * b``.

Quaternions are ``(x, y, z, w)``, Havok's order, and rotate a vector the usual way.
"""

import math
from dataclasses import dataclass

Vec3 = tuple[float, float, float]
Quat = tuple[float, float, float, float]


def add(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def sub(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def scale(a: Vec3, k: float) -> Vec3:
    return (a[0] * k, a[1] * k, a[2] * k)


def dot(a: Vec3, b: Vec3) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a: Vec3, b: Vec3) -> Vec3:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def length(a: Vec3) -> float:
    return math.sqrt(dot(a, a))


def qmul(a: Quat, b: Quat) -> Quat:
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


def qrot(q: Quat, v: Vec3) -> Vec3:
    u = (q[0], q[1], q[2])
    t = scale(cross(u, v), 2.0)
    return add(add(v, scale(t, q[3])), cross(u, t))


def qnormalize(q: Quat) -> Quat:
    n = math.sqrt(sum(c * c for c in q))
    return tuple(c / n for c in q) if n > 0 else (0.0, 0.0, 0.0, 1.0)


def qyaw(angle: float) -> Quat:
    """A turn of ``angle`` radians about +Y."""
    return (0.0, math.sin(angle / 2), 0.0, math.cos(angle / 2))


@dataclass(frozen=True)
class Xform:
    x: Vec3 = (1.0, 0.0, 0.0)
    y: Vec3 = (0.0, 1.0, 0.0)
    z: Vec3 = (0.0, 0.0, 1.0)
    origin: Vec3 = (0.0, 0.0, 0.0)

    @staticmethod
    def from_matrix(m) -> "Xform":
        """From the engine's 16 floats, row-major, translation in the last row."""
        return Xform(tuple(m[0:3]), tuple(m[4:7]), tuple(m[8:11]), tuple(m[12:15]))

    @staticmethod
    def from_quat(q: Quat, origin: Vec3 = (0.0, 0.0, 0.0)) -> "Xform":
        return Xform(qrot(q, (1.0, 0.0, 0.0)), qrot(q, (0.0, 1.0, 0.0)), qrot(q, (0.0, 0.0, 1.0)), origin)

    def apply(self, p: Vec3) -> Vec3:
        return add(add(add(scale(self.x, p[0]), scale(self.y, p[1])), scale(self.z, p[2])), self.origin)

    def rotate(self, v: Vec3) -> Vec3:
        return add(add(scale(self.x, v[0]), scale(self.y, v[1])), scale(self.z, v[2]))

    def then(self, b: "Xform") -> "Xform":
        """This transform, then ``b``."""
        return Xform(b.rotate(self.x), b.rotate(self.y), b.rotate(self.z), b.apply(self.origin))

    def inverse(self) -> "Xform":
        # The axes are orthonormal: the inverse rotation is the transpose.
        tx = (self.x[0], self.y[0], self.z[0])
        ty = (self.x[1], self.y[1], self.z[1])
        tz = (self.x[2], self.y[2], self.z[2])
        r = Xform(tx, ty, tz)
        return Xform(tx, ty, tz, scale(r.rotate(self.origin), -1.0))

    def yaw(self) -> float:
        """The heading of the transform's +Z axis about +Y, radians, as ``qyaw`` builds it."""
        return math.atan2(self.z[0], self.z[2])

    def is_upright(self, tolerance: float = 1e-3) -> bool:
        """Whether the rotation is a turn about +Y alone."""
        return abs(self.y[1] - 1.0) < tolerance
