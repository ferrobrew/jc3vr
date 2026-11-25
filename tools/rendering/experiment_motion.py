"""A moving-camera experiment: each configuration drives the camera along one periodic trajectory
about the game's frozen camera, and every capture is taken at the same phase of it.

The clock is held, so the world does not move; only the camera does, one trajectory step per
rendered frame. A capture is taken at the configured phase once a full cycle and the eight frames
before the phase have rendered, so the frames whose matrices the temporal effects read as the
previous frame's are the same trajectory frames in every mode.
"""

from dataclasses import dataclass

from tools.rendering.protocol import (
    ExpectedMotion,
    MotionRequest,
    PhaseCapture,
    ProtocolError,
)


class MotionConfigError(ValueError):
    pass


@dataclass(frozen=True)
class ExperimentMotion:
    request: MotionRequest
    phase: int

    def __post_init__(self):
        if not isinstance(self.request, MotionRequest):
            raise MotionConfigError("experiment: motion requires a motion request")
        if type(self.phase) is not int or not 0 <= self.phase < self.request.period_frames:
            raise MotionConfigError("experiment: motion.phase must be an integer within the period")

    @property
    def capture(self) -> PhaseCapture:
        return PhaseCapture(self.phase)

    @property
    def expected(self) -> ExpectedMotion:
        return ExpectedMotion(self.request, self.phase)

    @classmethod
    def from_toml(cls, name: str, value) -> "ExperimentMotion":
        """Read an experiment's `[motion]` table; the trajectory's case ID is the experiment name."""
        if not isinstance(value, dict):
            raise MotionConfigError("experiment: motion must be a table")
        required = {"period_frames", "phase"}
        optional = {"yaw_deg", "pitch_deg", "roll_deg", "right_m", "up_m", "forward_m"}
        missing = required - set(value)
        unknown = set(value) - required - optional
        if missing or unknown:
            raise MotionConfigError(f"experiment: motion is missing {sorted(missing)} or has "
                                    f"unknown keys {sorted(unknown)}")
        try:
            request = MotionRequest(name, value["period_frames"],
                                    *(value.get(key, 0.0) for key in sorted(optional, key=_ORDER.index)))
        except ProtocolError as error:
            raise MotionConfigError(f"experiment: motion: {error}") from error
        return cls(request, value["phase"])


_ORDER = ("yaw_deg", "pitch_deg", "roll_deg", "right_m", "up_m", "forward_m")


def validate_motion_capture(motion: ExperimentMotion, capture, anchor):
    """Check a capture's motion evidence against the trajectory and the configuration's anchor,
    returning the anchor."""
    if capture.visual.motion is None or capture.visual.pose is None:
        raise ProtocolError("experiment: a motion capture has no motion evidence")
    return capture.visual.motion.validate_capture(
        motion.request, motion.phase, anchor, capture.visual.pose,
        capture.comparison_inputs.camera, capture.predicted_display_time_ns)
