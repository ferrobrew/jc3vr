"""Headset-like eye rigs the visual harness substitutes for the located views.

The harness's simulated headset has symmetric, parallel eyes. A `headset` visual override names a
rig with asymmetric per-eye frusta and, for some, displays canted outward, and the payload
substitutes it for the located views in every mode. The presets are approximations of the shape of
each headset's views, not vendor data, and keep the aspect of the eye target each is meant for: the
harness's 320 by 360, or a square one for `beyond`.
"""

import math
from dataclasses import dataclass

from tools.rendering.protocol.base import (
    ProtocolError,
    euler_yxz_quaternion,
    OrientationOffset,
    object_value,
    quaternion_conjugate,
    quaternion_multiply,
    quaternion_normalize,
)

# How far a recorded value may stray from the preset it mirrors: the payload stores f32.
TOLERANCE = 1e-4


@dataclass(frozen=True)
class EyeGeometry:
    """One eye's view relative to the head: the field-of-view half-angles in degrees, `[left, right,
    up, down]` with left and down negative, and its turn from the head in degrees (yaw positive to
    the left, pitch positive up, then roll)."""
    fov_deg: tuple[float, float, float, float]
    yaw_deg: float
    pitch_deg: float
    roll_deg: float

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        if set(value) != {"fov_deg", "yaw_deg", "pitch_deg", "roll_deg"}:
            raise ProtocolError("visual protocol: invalid headset eye")
        fov = value["fov_deg"]
        if (not isinstance(fov, list) or len(fov) != 4 or
                any(type(item) not in (int, float) or not math.isfinite(item) for item in fov)):
            raise ProtocolError("visual protocol: invalid headset field of view")
        angles = [value[key] for key in ("yaw_deg", "pitch_deg", "roll_deg")]
        if any(type(item) not in (int, float) or not math.isfinite(item) for item in angles):
            raise ProtocolError("visual protocol: invalid headset eye orientation")
        return cls(tuple(float(item) for item in fov), *(float(item) for item in angles))

    def tangents(self):
        return tuple(math.tan(math.radians(angle)) for angle in self.fov_deg)

    def rotation(self):
        return euler_yxz_quaternion(OrientationOffset(self.yaw_deg, self.pitch_deg, self.roll_deg))

    def matches(self, other: "EyeGeometry") -> bool:
        return all(abs(a - b) <= TOLERANCE for a, b in zip(
            (*self.fov_deg, self.yaw_deg, self.pitch_deg, self.roll_deg),
            (*other.fov_deg, other.yaw_deg, other.pitch_deg, other.roll_deg)))


@dataclass(frozen=True)
class HeadsetGeometry:
    rig: str
    ipd_m: float
    eyes: tuple[EyeGeometry, EyeGeometry]

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        if set(value) != {"rig", "ipd_m", "eyes"}:
            raise ProtocolError("visual protocol: invalid headset geometry")
        if value["rig"] not in HEADSET_RIGS:
            raise ProtocolError(f"visual protocol: unknown headset rig {value['rig']!r}")
        ipd = value["ipd_m"]
        if type(ipd) not in (int, float) or not math.isfinite(ipd) or ipd <= 0:
            raise ProtocolError("visual protocol: invalid headset IPD")
        eyes = value["eyes"]
        if not isinstance(eyes, list) or len(eyes) != 2:
            raise ProtocolError("visual protocol: a headset geometry has two eyes")
        return cls(value["rig"], float(ipd), tuple(EyeGeometry.from_wire(eye) for eye in eyes))

    def matches(self, other: "HeadsetGeometry") -> bool:
        return (self.rig == other.rig and abs(self.ipd_m - other.ipd_m) <= TOLERANCE and
                all(a.matches(b) for a, b in zip(self.eyes, other.eyes)))

    def validate_eyes(self, eyes, world_scale: float) -> None:
        """Check the per-eye camera inputs a capture records against this rig: each eye's
        projection carries its field of view, its orientation relative to the centre head is its
        turn relative to the slerp midpoint of the two eyes' turns, and its offset is half the IPD
        along the head's horizontal axis in that frame."""
        rotations = [eye.rotation() for eye in self.eyes]
        centre = _slerp_half(*rotations)
        for index, (geometry, eye) in enumerate(zip(self.eyes, eyes)):
            tl, tr, tu, td = geometry.tangents()
            p = eye.projection_standard
            expected = {0: 2 / (tr - tl), 5: 2 / (tu - td), 8: (tl + tr) / (tr - tl),
                        9: (td + tu) / (tu - td)}
            for slot, value in expected.items():
                if abs(p[slot] - value) > TOLERANCE * max(1.0, abs(value)):
                    raise ProtocolError(f"visual protocol: eye {index}'s projection does not carry the "
                                        f"{self.rig} rig's field of view (element {slot}: {p[slot]} "
                                        f"against {value})")
            delta = quaternion_normalize(quaternion_multiply(quaternion_conjugate(centre), rotations[index]))
            if not _same_rotation(delta, eye.orientation_delta):
                raise ProtocolError(f"visual protocol: eye {index}'s orientation does not carry the "
                                    f"{self.rig} rig's turn ({eye.orientation_delta} against {delta})")
            side = -0.5 if index == 0 else 0.5
            offset = _rotate(quaternion_conjugate(centre), (side * self.ipd_m * world_scale, 0.0, 0.0))
            if any(abs(a - b) > TOLERANCE for a, b in zip(eye.offset, offset)):
                raise ProtocolError(f"visual protocol: eye {index}'s offset does not carry the "
                                    f"{self.rig} rig's IPD ({eye.offset} against {offset})")


def _mirrored(rig, ipd_m, fov_deg, yaw_deg, pitch_deg, roll_deg) -> HeadsetGeometry:
    left, right, up, down = fov_deg
    return HeadsetGeometry(rig, ipd_m, (
        EyeGeometry((left, right, up, down), yaw_deg, pitch_deg, roll_deg),
        EyeGeometry((-right, -left, up, down), -yaw_deg, pitch_deg, -roll_deg)))


def _stress() -> HeadsetGeometry:
    geometry = _mirrored("stress", 0.070, (-60.0, 35.0, 49.0, -58.0), 12.0, 1.0, 1.0)
    left, right = geometry.eyes
    return HeadsetGeometry(geometry.rig, geometry.ipd_m,
                           (left, EyeGeometry(right.fov_deg, right.yaw_deg, -left.pitch_deg, right.roll_deg)))


# Mirrors `HeadsetRig::geometry` in `payload/src/testing/visual/headset.rs`.
HEADSET_RIGS = {
    "index": _mirrored("index", 0.064, (-54.0, 51.0, 55.8, -55.8), 5.0, 0.0, 0.0),
    "quest": _mirrored("quest", 0.063, (-50.0, 43.0, 48.0, -52.0), 0.0, 0.0, 0.0),
    "stress": _stress(),
    "beyond": _mirrored("beyond", 0.063, (-51.0, 45.0, 48.2, -48.2), 0.0, 0.0, 0.0),
}


def _slerp_half(a, b):
    """The rotation halfway between `a` and `b` along the shorter arc."""
    if sum(x * y for x, y in zip(a, b)) < 0:
        b = tuple(-item for item in b)
    return quaternion_normalize(tuple(x + y for x, y in zip(a, b)))


def _same_rotation(a, b) -> bool:
    return (all(abs(x - y) <= TOLERANCE for x, y in zip(a, b)) or
            all(abs(x + y) <= TOLERANCE for x, y in zip(a, b)))


def _rotate(quaternion, vector):
    x, y, z, w = quaternion_multiply(quaternion_multiply(quaternion, (*vector, 0.0)),
                                     quaternion_conjugate(quaternion))
    return (x, y, z)
