"""Typed values for the visual harness's `visual_stage` request, which moves the player to a
location, spawns objects, and lets the world settle before a configuration freezes it."""

import json
import math
from dataclasses import dataclass, field

from tools.rendering.protocol.base import (
    AckOutcome,
    Command,
    ProtocolError,
    integer,
    object_value,
    text,
    valid_request_id,
)

# Mirrors `MAX_SETTLE_SECONDS` and `MAX_SPAWNS` in `payload/src/testing/stage.rs`.
MAX_SETTLE_SECONDS = 300.0
MAX_SPAWNS = 16


def _position(value, context) -> tuple[float, float, float]:
    if (not isinstance(value, (list, tuple)) or len(value) != 3 or
            any(type(item) not in (int, float) or not math.isfinite(item) for item in value)):
        raise ProtocolError(f"visual stage: {context} must be three finite numbers")
    return tuple(float(item) for item in value)


def _yaw(value, context) -> float:
    if type(value) not in (int, float) or not math.isfinite(value):
        raise ProtocolError(f"visual stage: {context} must be a finite number")
    return float(value)


@dataclass(frozen=True)
class Placement:
    """A world position and a heading in degrees about world up, from +Z toward +X."""
    position: tuple[float, float, float]
    yaw_deg: float = 0.0

    def __post_init__(self):
        object.__setattr__(self, "position", _position(self.position, "position"))
        object.__setattr__(self, "yaw_deg", _yaw(self.yaw_deg, "yaw_deg"))

    def toml_body(self) -> str:
        return (f"position = {json.dumps(list(self.position))}\n"
                f"yaw_deg = {json.dumps(self.yaw_deg)}\n")


@dataclass(frozen=True)
class StageSpawn:
    tags: str
    placement: Placement

    def __post_init__(self):
        if not isinstance(self.tags, str) or not self.tags.strip() or "\0" in self.tags:
            raise ProtocolError("visual stage: a spawn needs a nonempty tag string")


@dataclass(frozen=True)
class StageRequest:
    id: str
    settle_seconds: float
    location: Placement | None = None
    spawns: tuple[StageSpawn, ...] = ()
    # Set and pinned before the teleport, without holding the day cycle, so the weather has run for
    # the whole settle. An experiment fills it from its scene's environment.
    environment: "EnvironmentRequest | None" = None
    command: Command = field(default=Command.STAGE, init=False)
    # The channel validates these on configuration acknowledgements only.
    pose = None
    motion = None

    def __post_init__(self):
        if not valid_request_id(self.id):
            raise ProtocolError("visual stage: invalid request ID")
        if (type(self.settle_seconds) not in (int, float) or not math.isfinite(self.settle_seconds) or
                not 0 <= self.settle_seconds <= MAX_SETTLE_SECONDS):
            raise ProtocolError(f"visual stage: settle_seconds must be within [0, {MAX_SETTLE_SECONDS}]")
        object.__setattr__(self, "settle_seconds", float(self.settle_seconds))
        object.__setattr__(self, "spawns", tuple(self.spawns))
        if len(self.spawns) > MAX_SPAWNS:
            raise ProtocolError(f"visual stage: at most {MAX_SPAWNS} spawns")
        if self.location is None and not self.spawns:
            raise ProtocolError("visual stage: a stage names a location, a spawn, or both")

    def to_toml(self) -> str:
        result = (f"command = {json.dumps(self.command.value)}\n"
                  f"id = {json.dumps(self.id)}\n\n"
                  "[stage]\n"
                  f"settle_seconds = {json.dumps(self.settle_seconds)}\n")
        if self.location is not None:
            result += "\n[stage.location]\n" + self.location.toml_body()
        if self.environment is not None:
            result += self.environment.to_toml().replace("[environment]", "\n[stage.environment]", 1)
        for spawn in self.spawns:
            result += ("\n[[stage.spawn]]\n"
                       f"tags = {json.dumps(spawn.tags)}\n" + spawn.placement.toml_body())
        return result

    @classmethod
    def from_toml(cls, request_id: str, value, context="scene.stage") -> "StageRequest":
        """Read an experiment's `[scene.stage]` table into a request with the given ID."""
        if not isinstance(value, dict):
            raise ProtocolError(f"visual stage: {context} must be a table")
        unknown = set(value) - {"settle_seconds", "location", "spawn"}
        if unknown:
            raise ProtocolError(f"visual stage: {context} has unknown keys {sorted(unknown)}")
        location = value.get("location")
        if location is not None:
            location = _placement(location, f"{context}.location")
        spawns = value.get("spawn", [])
        if not isinstance(spawns, list):
            raise ProtocolError(f"visual stage: {context}.spawn must be an array of tables")
        parsed = []
        for index, spawn in enumerate(spawns):
            item = f"{context}.spawn[{index}]"
            if not isinstance(spawn, dict) or "tags" not in spawn:
                raise ProtocolError(f"visual stage: {item} must be a table with tags")
            parsed.append(StageSpawn(spawn["tags"], _placement(
                {key: item_value for key, item_value in spawn.items() if key != "tags"}, item)))
        if "settle_seconds" not in value:
            raise ProtocolError(f"visual stage: {context} needs settle_seconds")
        return cls(request_id, value["settle_seconds"], location, tuple(parsed))


def _placement(value, context) -> Placement:
    if not isinstance(value, dict) or "position" not in value or set(value) - {"position", "yaw_deg"}:
        raise ProtocolError(f"visual stage: {context} must be a table of position and yaw_deg")
    try:
        return Placement(value["position"], value.get("yaw_deg", 0.0))
    except ProtocolError as error:
        raise ProtocolError(f"{error} in {context}") from error


@dataclass(frozen=True)
class StageAck:
    id: str
    player_position: tuple[float, float, float] | None
    distance_m: float | None
    spawns_submitted: int
    spawns_present: int
    outcome: AckOutcome = field(default=AckOutcome.STAGED, init=False)

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        position = value.get("player_position")
        if position is not None:
            position = _position(position, "player_position")
        distance = value.get("distance_m")
        if distance is not None and (type(distance) not in (int, float) or not math.isfinite(distance) or
                                     distance < 0):
            raise ProtocolError("visual stage: invalid distance_m")
        return cls(text(value, "id"), position, None if distance is None else float(distance),
                   integer(value, "spawns_submitted", minimum=0),
                   integer(value, "spawns_present", minimum=0))
